# pinion seed prompt — 매 세션 첫 입력

> R663.5 (2026-05-24) vision 정정 후 canonical baseline. R664+ 각 라운드 종료 시 "직전 세션 결과" + "다음 텍스트북 캐논" + "watch out" + "lessons" 절 갱신.

---

【불변 운영 원칙】 (첫 7줄 — 매 세션 동일)
- 비용 무시. 항상 장기적으로 올바른 textbook-canonical 선택
- **진짜 북극성 = AAA game shippable + Unreal-class editor self-hosted in pinion itself, AI-introspection 1st-class through every phase.** 4-phase progression: A. Foundation (현재 70%, R655-R667 todomvc+settings panel) → B. Professional GUI (Qt/Flutter/Compose-class + multi-window + DCC widget catalog, R700+) → C. Game engine substrate (§2 #4 immediate-mode game loop ↔ retained widget tree dual execution + 3D + assets + physics + audio + PBR, R1000+) → D. AAA game maker (editor self-hosted in pinion + visual scripting + Nanite/Lumen-class rendering + multiplayer netcode, R2500+). 현재 가중 진척 ~6%. R664-R667 cascade 후 ~8%. R667 = Phase A 종료 = 진짜 북극성의 ~5%, **NOT 북극성 도달**
- 부채 즉시 상환. 라운드 안 발견 부채 inline 청산, carry 영원 누적 금지. 이전 라운드 honest 약점 → 다음 라운드 inline 청산 mandatory. 외부 의존 (vendor/sce upstream, 환경) 만 honest carry 정당
- 라운드 자동 선택. 세션 80% 까지 계속
- "부채는 양파다" — 청산 시 새 부채 surface 정직 받아들임
- 1 commit = 1 round = 1 atomic Mnemosyne entry
- 사용자 명시 동의 없으면 git push 금지 (CLAUDE.md 영구 원칙). "진행" / "continue" / "go" 는 push 권한 아님

【진입 시 필독 순서】
1. `docs/SEED_PROMPT.md` (이 파일 — R664+ matters 의 baseline)
2. `docs/GENERATED.md` §1 Vision (R663.5 정정: 4-phase) + §2 invariants (R663.5 #4 elaboration)
3. `mnemosyne://concepts/overview` + anti-patterns + atomic-store + frozen-ledger
4. `CLAUDE.md` (R663.5 H1 + Project identity + #4 elaboration)
5. `~/.claude/CLAUDE.md` + `COMMIT_FORMAT.md`
6. `git log --oneline -30` (R635-R663.5)
7. `memory/MEMORY.md` — 특히:
   - `[[true-north-star-phases]]` ★ R663.5 신규, 가장 중요
   - `[[project_scope_game_engine]]` ★ R663.5 정정
   - `[[r663-double-click-primitive]]` ★ R664 substrate parent
   - `[[r662-sce004-access-child-invoke]]`
   - `[[r661-doc-compression]]`
   - `[[r660-todomvc-debt-clearance]]`
   - `[[r47-class-incident-prevention]]` (R664 native paint detection 룰)
   - `[[abstraction-needs-second-consumer]]` (R664 paint-side 2nd consumer 룰)
   - `[[substrate-incompleteness-signal]]`
   - `[[textbook-long-term-correct]]`
   - `[[owner-cache-typed-key]]` (R664 use_editing_id 패턴)
   - `[[multi-external-substrate-extra-externals-pattern]]` (5th External candidate)
   - `[[ai-first-rpc-introspection-obligation]]`
   - `[[sce-priority-over-pinion]]` / `[[sce-upstream-debts]]` (SCE-004)
   - `[[r650-widget-tag-walk-back]]`

【직전 5 세션 결과 — honest 누적 평가】

land 완료 (5 commits, d5325d5 → daf2a99 → 2d262ad → d8e6810 → bde04f7):

- **R660** (substrate debt-clearance) `d5325d5`:
  - TodoFilterExternal (~199 LOC bespoke) → RadioGroupExternal Option β walk-back
  - W3C ARIA roving-tabindex (Tab/Arrow/Home/End/Space)
  - M3 state-layers: filter buttons (Hover 0.08 / Pressed 0.12) + scrollbar thumb (Hover 0.08 / Drag 0.16)
  - ScrollBarInteractionSignal Owner::cache + use_scrollbar_interaction
  - scene/drag RPC method + DeferredInput::Drag + tools/rpc_verify.py drag()
  - composite_tag 5-of-5 framework consumer maturity
  - 40-assertion R660 demo + 3303 workspace tests pass

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

honest 평가 누적 — R663 honest 부채 5개 (R664 inline 청산 mandatory):

1. **Native paint-side double-click detection 부재 (R663 carry)** — R663 substrate 가 RPC primary path 만 fire. winit MouseInput 의 consecutive Pressed 300ms/5px threshold detection 부재. **R664 가 paint-side 2nd consumer 가 되므로 `[[abstraction-needs-second-consumer]]` 룰 satisfy** — R664 inline mandatory. crates/pinion-shell/src/substrate.rs 의 mouse_pressed 확장 (last_click cache + 300ms/5px window + W3C detail count)
2. **ScrollBarInteractionSignal u8-wrapper stop-gap** — SCE-004 upstream 대기. R664 직접 청산 불가 (외부 의존). honest carry
3. **todo_item / delete / toggle AT-click wire 부재 (R662 carry)** — parent_tag 가 substrate 로 land 했지만 todomvc 는 filter 만 wire. R664 inline 청산 candidate
4. **scene/invoke v0 primary-only** — multi-External path syntax (R690+)
5. **text-decoration strikethrough 3rd consumer 부재** — R664 view_field swap 시점에서 completed item 의 strikethrough 가 자연스러우면 1st paint primitive consumer 등장. **R664 inline mandatory** ([[r47-class-incident-prevention]] paint primitive 룰)
6. **R657 view_field 3rd consumer (R664 todomvc per-item edit-mode)** — R664 가 진짜 3rd consumer ROI 확정 시점

framework substrate completeness 부채 (R664 청산 candidate):
- **use_text_edit_state per-item Owner::cache 2nd consumer 검증** — per-item key (format!("todo_edit#{id}")) 가 R664 첫 다중-인스턴스 consumer. dynamic key (`&'static str` vs `String`) 지원 여부 substrate-incompleteness signal 가능성

carry honest (외부 의존, R664 미청산):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- TUI parity (§6 #6 cascade)
- Figma API token (영구)
- WIN_H 480 magic (flex-grow primitive 누락)

진척도 (R663.5 후) — **진짜 northern-star 대비**:

| Phase | 비중 | 현재 |
|---|---:|---:|
| A. Foundation (§1-§4 + 첫 composed apps) | 5% | 70% |
| B. Professional GUI (Qt/Flutter/Compose-class + multi-window) | 25% | 10% |
| C. Game engine substrate (§2 #4 dual execution + 3D + ...) | 35% | 0% |
| D. AAA editor self-hosted | 35% | 0% |

**가중 진척 = 5%×70% + 25%×10% + 35%×0% + 35%×0% = ~6%**

R664-R667 cascade 후 Phase A 95% → ~7-8%. Phase B 진입 (R700+) 이 진짜 northern-star 의 +5% 가속. Phase C/D 가 진짜 mass (35%+35% = 70% of work).

가시 결과:
- `./target/release/todomvc` — Tab/Arrow/Home/End/Space filter cycle + M3 hover/press layers + scrollbar drag
- `python3 tools/demos/todomvc_r660.py` (40 assertion, 6.57s)
- `python3 tools/demos/double_click_r663.py` (smoke, 1.97s)

【북극성 명확화 — Phase A finalisation (R664-R667) 의미】

§1 Vision (R663.5 정정) + §2 7 invariants 가 가리키는 4-phase progression:

(a) **R664 = todomvc edit-in-place + R663 paint-side 2nd consumer 청산** — R663 substrate consumer 등장. view_field 3rd consumer = R657 lift ROI 확정 시점. text-decoration strikethrough = 1st paint primitive consumer (R47-class). Phase A 진척 70% → ~80%

(b) **R665 = External(opaque) persistence** — LocalStorage / JSON via pinion-platform-storage crate. §3 Effect/External capability boundary 정통 escape hatch 첫 실증. Phase A 진척 80% → ~85%

(c) **R666 = AI driving 12+ step end-to-end** — composite path routing, multi-state introspection workflow. scene/invoke v1 multi-External path syntax (R690+ candidate, 또는 R666 inline). Phase A 진척 85% → ~90%

(d) **R667 = 2nd composed app (settings panel)** — view_vertical_scrollbar 4th consumer + view_field 4th consumer = substrate ROI curve fully positive 검증. **Phase A 종료** = 진짜 북극성의 ~7-8% 도달

(e) **R700+ = Phase B 진입** — Multi-window substrate 첫 라운드. winit 이미 multi-window 지원, `pinion-shell::WindowManager` + `Scene::Window` enum. DevTools / Inspector 가 첫 multi-window consumer. **이 시점이 진짜 framework 가 "professional tool 가능" 으로 도약**

(f) **R1000+ = Phase C 진입 = §2 #4 의 진짜 구현** — `ImmediateModeNode` primitive + game-loop infrastructure (60-144fps lockstep + delta time + frame budget cap) + retained↔immediate runtime switch per `Scene::Container` subtree. 동일 binary 안 settings panel = retained / 3D viewport = immediate

(g) **R2500+ = Phase D 진입** — Editor self-hosted in pinion itself. Unreal-class IDE 작성 시작. 진짜 northern-star 의 본격 진입

【다음 텍스트북 캐논 — R664 atomic 청산 8개】

R664 = **edit-in-place + R663 paint-side consumer 청산 라운드**

핵심 청산 항목 (모두 inline mandatory):

(1) **Native paint-side double-click detection** — R663 substrate 의 2nd consumer 등장. `crates/pinion-shell/src/substrate.rs` 의 `mouse_pressed` 확장 (`last_click {timestamp: Instant, x: f64, y: f64, button: PointerButton}` 추적 + 300ms / 5px threshold). 2nd press in window → detail=2 marker. **결정**: W3C UIEvent canonical = `V::apply_pointer_down(scene, focused, button, detail: u8)` trait method signature 확장 (detail count, 미래 triple-click 등 detail 증가만, 별 trait method 폭증 회피). R663 RPC primary path 와 동일 추상화 수렴

(2) **`use_editing_id` Owner::cache hook + per-item TextEditState multi-instance** — `pub fn use_editing_id() -> Rc<Signal<Option<u64>>>`. per-item: `use_text_edit_state(format!("todo_edit#{id}"))` — R56.1.b.1 substrate 의 multi-instance load 첫 검증. **결정 트리**: `use_text_edit_state(key: &'static str)` 가 dynamic key 미지원이면 substrate 확장 (`use_text_edit_state_dynamic(key: String)` 또는 `(TypeId, String)` slot model). `[[owner-cache-typed-key]]` 의 generic key axis 가 textbook canonical

(3) **조건부 row rendering in build_todos_list** — row.id == editing_id → `tf_paint::view_field(format!("todo_edit#{id}"), interaction, caret_byte, &theme, &TextFieldStyle::m3_filled(), item.text.as_str())`. 즉시 R657 view_field 3rd consumer 도달 = R657 lift ROI 확정 시점

(4) **Enter commit / Escape cancel / blur commits — apply_key 확장** — focused 가 todo_edit#<id> 형식이면 edit mode. Enter → text_state.text() 로 todos.set_with 갱신 + use_editing_id.set(None) + text_state clear. Escape → use_editing_id.set(None) + text_state clear (원래 text 유지). blur (focus_set 다른 tag) → commits (TasteJS canonical = blur commits; textbook = 사용자 의도 보존)

(5) **신규 TextField 자동 focus — programmatic focus mgmt** — double-click on todo_item#<id> → editing_id.set(Some(id)) + focus_set(format!("todo_edit#{id}")) + text_state.set_text(item.text). focus_set 가 dynamic tag 지원 검증 시점 (또 substrate-incompleteness 가능성)

(6) **TodoEditExternal (5th ExtraExternal)** — double-click detail=2 paint event 처리. invoke("send", "<id>:PointerDown") 가 detail=2 marker 시 editing_id 활성화 + auto-focus + text seed. detail=1 은 no-op (single-click 은 delete/toggle 처리). Single Responsibility 원칙 → TodoEditExternal 신설 정통

(7) **text-decoration strikethrough substrate** — 1st paint primitive consumer = R664 inline mandatory ([[r47-class-incident-prevention]]). `pinion-core::style::TextStyle::with_strikethrough(bool)` field 추가 + vello paint backend (strike-through line) + TUI paint backend (Unicode combining stroke) emission. completed item 의 text 가 strikethrough = TasteJS canonical

(8) **AT-action wire 확장 — todo_item / delete / toggle 도 parent_tag 분기 추가** (R662 carry 청산) — filter wire 패턴 mirror. Click/Default/Focus 각 action 의 send wire. R662 parent_tag substrate 의 5-of-5 framework consumer push

R664 visible deliverable:
- `cargo run -p todomvc` — 더블클릭 on item text → editable TextField + auto-focus + caret blink. Type 수정 → Enter 저장 / Esc 취소 / blur 저장. completed item 의 text 가 strikethrough

RPC demo R664 = ≥ 30 assertion 시나리오:
- paint-side double-click (scene/double_click + native detection unit test)
- editing_id 활성화 검증
- set_text 로 텍스트 수정
- Enter commit + todos 갱신
- Escape cancel + 원래 text 유지
- blur commits (Option β decision pin)
- completed strikethrough paint snapshot
- AT-click on todo_item#<id> → edit mode 진입 (parent_tag wire)

honest LOC 예측: ~+970 LOC net (substrate-heavy round; doc-density R661 baseline 유지)
- (1) native paint double-click: +150 LOC
- (2) use_editing_id + per-item TextEditState (dynamic key 필요 시 +200 substrate): +50-200 LOC
- (3) 조건부 row rendering: +50 LOC
- (4) Enter / Escape / blur: +80 LOC
- (5) auto-focus mgmt: +50 LOC
- (6) TodoEditExternal: +180 LOC
- (7) strikethrough substrate: +200 LOC
- (8) AT-action wire 확장: +60 LOC

실측 후 honest 정정 의무. **R660-R663 누적 substrate work 대비 R664 = application axis 우선 (northern-star 직접 진행)** — substrate 확장은 R663 carry 청산 + view_field 3rd consumer ROI 도달 시 필연

진척도 +2-3%p 예상 (~6% → ~8% 진짜 northern-star 대비)

【R665-R667 Phase A 완성 cascade】

R665 — External(opaque) persistence (LocalStorage / JSON)
- pinion-platform-storage crate 신설 (browser localStorage / native file)
- todos + filter mode + editing_id 영구화
- schema versioning (dehydrate/rehydrate)
- §3 Effect/External 정통 escape hatch 첫 실증

R666 — AI driving 12+ step end-to-end
- scene/invoke v1 multi-External path syntax (R690+ carry, 또는 inline)
- 12+ step workflow: add 5 → toggle 3 → filter Active → edit 1 → commit → persist → reload → verify
- 모든 step 이 AI introspection 으로 self-verify

R667 — 2nd composed app (settings panel) — Phase A 종료
- view_vertical_scrollbar 4th consumer
- view_field 4th consumer
- R657/R659/R660 substrate ROI curve fully positive
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

【watch out — 영구 + 누적 (R663.5 후 갱신)】

기존 누적 + R663.5 land 청산:
- ✓ R660 청산: visible scrollbar peer + composite_tag 5-of-5
- ✓ R660 청산: filter kbd nav (Option β walk-back)
- ✓ R660 청산: filter / scrollbar M3 state-layers
- ✓ R660 청산: scene/drag substrate
- ✓ R661 청산: doc-heavy LOC overshoot (R658 +1051 / R659 +1980 / R660 +800 → R661 -796 baseline)
- ✓ R662 청산: SCE-004 upstream debt registered + doc-anchor
- ✓ R662 청산: WidgetA11y::access_child_invoke parent_tag substrate
- ✓ R662 청산: todomvc filter AT-action wire
- ✓ R663 청산: scene/double_click framework primitive
- ✓ R663.5 청산: §1 Vision + §2 #4 + CLAUDE.md + memory 5-layer northern-star 정정. axis 선택 anchor 재설정

R663 carry (R664 inline 청산 mandatory):
- Native paint-side double-click detection (winit 300ms/5px)
- text-decoration strikethrough (1st paint primitive consumer = R664 inline mandatory)
- todo_item/delete/toggle AT-click wire (R662 parent_tag 의 추가 consumer)

영구 carry (외부 의존):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC
- SCE-002 (consumer-injectable derive list) — SCE-004 와 같은 axis
- scene/invoke v0 primary-only (R690+ multi-External path — R666 inline 청산 candidate)
- WIN_H 480 magic (flex-grow primitive)
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT)
- Figma API token
- TUI parity (§6 #6 cascade)

【R660-R663.5 lessons — 명시】

- **R663.5 정정의 가장 큰 lesson — Vision spec 명시가 모든 라운드 axis 선택의 anchor.** R660-R663 동안 "R667 settings panel = 북극성" misdirection 으로 substrate / process maturity / framework debt 만 진행. 진짜 northern-star (AAA + editor self-hosted) 가 spec / CLAUDE / memory / seed prompt 5-layer 어디에도 명시 안 됨. **axis 선택 시 매번 self-check: 이 라운드가 진짜 northern-star (4-phase 종착) 에 얼마나 가까이 가는가?**
- **Substrate-first ordering 정통** — R663 framework-first double-click (vs todomvc inline) 이 [[r47-class-incident-prevention]] textbook. R664 가 substrate-consumer 로 정직
- **Native paint detection 의 2nd-consumer timing** — R663 가 1st RPC consumer, R664 todomvc 가 paint-side 2nd consumer 등장 → [[abstraction-needs-second-consumer]] 룰 satisfy → native detection inline 청산 mandatory
- **Doc compression baseline (R661) effective** — R660 +800 overshoot 후 R661 -796 청산 zero behaviour change. process maturity 라운드 분리 = substrate refactor 안전
- **SCE upstream debt 의 doc-anchor pattern** — R662 stop-gap 의 retire path 를 코드 doc 에 명시. 미래 Forge serde derive land 시 automatic retire
- **parent_tag substrate 의 multi-composite 일반화** — R662 access_child_invoke 확장이 R665+ 추가 composite (persist controls / R667 settings panel sections) 시 자동 활용
- **5-of-5 substrate maturity** — composite_tag 의 4 application + 2 framework composite. R664 의 TodoEditExternal = 6th consumer. mature substrate 는 sublinear 비용 증가

【명시적 금지】

- README.md / docs.rs / user guide proactive 생성 금지
- Material Symbols / 외부 폰트 vendor commit 금지
- macro magic / 숨겨진 동작 channel 금지
- vendor/sce 직접 수정 금지 (SCE-004 등록 후 PR 경로만 정통)
- TodoMVC 외 다른 첫 composed app 변경 금지 (R667 까지)
- process round (0 LOC code change) 연속 2 이상 금지 — R663.5 vision 정정 1회 + 미래 doc compression 라운드 1회 까지만 정통
- visible deliverable 없는 라운드 금지 (process maturity / vision 정정 라운드는 예외)
- **R664 inline 청산 누락 금지** (R663 honest 약점 5+1: native paint detection / strikethrough / AT-click 확장)
- doc-heavy LOC 정당화 자동 허용 금지 — R661 baseline 유지
- pinion-widget-paint::toggle.rs / slider.rs 신규 모듈 추가 금지
- **R665 (persistence) 진입 전 R664 부채 청산 100% 완료 mandatory**
- text-decoration strikethrough 의 R670+ 캐리 금지 — R664 1st paint primitive consumer 등장 시 substrate mandatory
- **§1 vision 추가 권유 금지 / 에셋·물리·오디오 spec add 권유 금지 (Phase A 완료까지)** — `[[project_scope_game_engine]]` 단기 룰. Phase B-D 진입은 R667 (Phase A 종료) 이후
- Phase B/C/D 라운드 (R700+/R1000+/R2500+) 의 axis 를 R664-R667 안에서 시작 금지 — forward-compatible 설계 검토만

【프롬프트 사용법】
새 세션 시작 시 이 파일 전체 입력 (또는 "@docs/SEED_PROMPT.md 읽고 진행"). 첫 7줄 (불변 운영 원칙) 매 세션 동일. "직전 5 세션 결과" + "다음 텍스트북 캐논" + "watch out" + "lessons" 매 세션 갱신.

【시작 명령】

R664 edit-in-place + R663 paint-side consumer 청산 라운드 자동 진행. 8개 atomic land:

(1) Native paint-side double-click detection — `pinion-shell::substrate` mouse_pressed 확장 (last_click cache + 300ms/5px threshold + W3C detail count); R663 RPC primary path 와 통합 dispatch
(2) `use_editing_id` Owner::cache hook + per-item TextEditState multi-instance (필요 시 substrate 동적 key 확장)
(3) build_todos_list 조건부 row rendering — editing_id 매칭 row 만 tf_paint::view_field 로 swap. view_field 3rd consumer 도달
(4) apply_key 확장 — edit mode 에서 Enter commit / Escape cancel / blur commits
(5) Auto-focus mgmt — double-click on todo_item#<id> → editing_id 활성화 + focus_set + text seed
(6) TodoEditExternal (5th ExtraExternal) — double-click detail=2 paint event 처리
(7) text-decoration strikethrough substrate — TextStyle::with_strikethrough + vello / TUI paint decoration emission (1st paint primitive consumer = R664 inline mandatory)
(8) todo_item / delete / toggle AT-action wire — parent_tag 분기 추가 (R662 substrate 5th consumer)

visible: `cargo run -p todomvc` — 더블클릭 item text → editable TextField + auto-focus + caret blink. Type 수정 → Enter 저장 / Esc 취소 / blur 저장. completed item 의 text 가 strikethrough.

RPC verify demo R664 = ≥ 30 assertion. honest LOC 예측: ~+970 LOC net. 실측 후 honest 정정 의무.
