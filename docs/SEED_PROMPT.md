# pinion seed prompt — 매 세션 첫 입력

> **R697 land (2026-05-29, commit `4afcae0`)** — **multi-open WAI-ARIA APG Accordion = R696 Disclosure substrate 의 2nd consumer**. 새 core widget 0개 — 순수 composition round(substrate 검증, [[substrate-incompleteness-signal]]). **핵심**: (1) `examples/hello-accordion` = N=3 `DisclosureExternal` 을 `WidgetCore::create_extra_externals`(R55.D.5 §5.45 homogeneous-cluster path; settings-panel 6× CheckboxExternal 과 동형)로 합성 — section 0 = `create_external`, 1·2 = extras, 헤더마다 `accordion_sec_{i}` 태그 → InputRouter depth-first walk 가 헤더별 클릭 독립 라우팅. (2) **multi-open = APG 기본값**: 한 섹션 토글이 다른 섹션 안 닫음 → cross-section 조정 0, reducer 0, 신규 substrate 0. single-open(exclusive)은 RadioGroup 식 framework-owned mutual exclusion → 2nd consumer 까지 defer([[abstraction-needs-second-consumer]]). (3) 키보드: `focusable_tags()` 가 N 헤더 전부 열거(각자 Tab stop — RadioGroup 의 single-tab-stop roving 과 대조); `apply_key` 가 Space/Enter 로 focused 섹션 KeyboardActivate + ArrowDown/Up(wrap)·Home/End 로 헤더 간 **focus** 이동(R664 `focus_request` mailbox; winit·RPC 양 arc 에서 `handle_tail`→`drain_focus_request`로 같은 frame 적용). (4) a11y: N개 flat `AriaRole::Button`(부모 없음 — WAI-ARIA 에 accordion role 無) + 각 `with_expanded`(R696 AccessNode 필드) + focused; 이름은 헤더 summary name-from-contents enrich. (5) `view_disclosure`(R696) 섹션마다 bit-identical 재사용 — paint 무변, 패널 body 는 expanded 일 때만 scene 존재. **검증**: `cargo test --workspace` -j2 0-fail(accordion 15 unit: composition/multi-open/focus-roving/a11y), clippy `-D pedantic` clean, `r697_accordion.py` >40-assert PASS(multi-open 독립성·동시 3-open·per-section click/invoke/intervene·Space+Enter focused-only·Arrow/Home/End focus roving via `focus/get`) + 전체 sweep 54/54, Mnemosyne `R697`(ledger 577→578; T1 +0; RT 1/1; GENERATED sync; impact [5.16,5.38,5.39,5.40,5.45,5.50]). **honest carry**: 헤더 visual focus-ring 無 — R694 composite-paint focus-ring carry 상속(view_disclosure ring 미그림); focus 는 AT-reported(access_node focused) + RPC-verifiable(`focus/get`)이나 시각 표시는 focus-ring substrate 라운드에서 카탈로그 일괄 점등. **환경**: `-j2`(CARGO_BUILD_JOBS=2) 필수(OOM); commit hook clippy 도 동일.
>
> **다음 세션 진입**: `load` 단독 입력. R698 = Phase B widget catalog 계속. 후보 = **`widget_state_name!` adoption sweep**(R696.A carry — checkbox/radio/button/slider External introspect 가 아직 local `*_state_name` fn; State::as_name() 로 라우팅 = behaviour-preserving low-cost cleanup) / **Accordion single-open variant**(framework-owned single-expand, 2nd consumer) / **Drawer**(overlay-dismiss substrate-first) / **Table**(virtualization substrate-first, §5.27 VirtualList comment-only) / DatePicker / ColorPicker. 진입 시 class(command/selection/descriptive/container) + substrate 전제 먼저 grep(작은 substrate 는 1st consumer 와 함께 land, 큰 substrate 는 substrate-first 라운드).
>
> R697 가중 진척: Phase A 97% + Phase B 25% × ~95% + Phase C 35% × ~12% = 북극성 가중 **~38.9%** (catalog widget +1 Accordion = Disclosure substrate 2nd consumer; 신규 core widget 0).

> **이전 라운드 land 기록 (R1 ~ R688.A)**: `git log --oneline` + `docs/GENERATED.md` (Mnemosyne 렌더 changelog) 이 single source of truth. 이번 SEED 정리(2026-05-28, R689 세션 후속)에서 비대화(`/load` 시 auto-compaction → thinking-block 손상 → API 400 유발)를 막기 위해 과거 land 블록 + DONE plan 절 + 직전-N-세션 상세 기록을 SEED 에서 **제거** — 전부 git 히스토리 + GENERATED.md 에 무손실 보존됨. 특정 라운드 상세가 필요하면 `git show <hash>:docs/SEED_PROMPT.md` 또는 `git log -S"<키워드>" docs/SEED_PROMPT.md` 로 조회.

【불변 운영 원칙】 (매 세션 동일)
- 비용 무시. 항상 장기적으로 올바른 textbook-canonical 선택
- **진짜 북극성 = AAA game shippable + Unreal-class editor self-hosted in pinion itself, AI-introspection 1st-class through every phase.** 4-phase progression: A. Foundation (현재 ~97%) → B. Professional GUI (Qt/Flutter/Compose-class + multi-window + DCC widget catalog, R700+) → C. Game engine substrate (§2 #4 immediate-mode game loop ↔ retained widget tree dual + 3D + assets + physics + audio + PBR, R1000+) → D. AAA game maker (editor self-hosted + visual scripting + Nanite/Lumen-class rendering + multiplayer netcode, R2500+). 현재 가중 진척 ~38.5%. R667 = Phase A 종료 = **NOT 북극성 도달**
- 부채 즉시 상환. 라운드 안 발견 부채 inline 청산, carry 영원 누적 금지. 이전 라운드 honest 약점 → 다음 라운드 inline 청산 mandatory. 외부 의존 (vendor/sce upstream, 환경) 만 honest carry 정당
- 라운드 자동 선택. 세션 80% 까지 계속. 방향 AskUserQuestion 으로 묻지 말 것 ([[round-direction-auto-select-no-ask]])
- "부채는 양파다" — 청산 시 새 부채 surface 정직 받아들임
- 1 commit = 1 round = 1 atomic Mnemosyne entry
- 사용자 명시 동의 없으면 git push 금지 (CLAUDE.md 영구 원칙). "진행" / "continue" / "go" 는 push 권한 아님

【다음 세션 진입 (single-command entry)】

새 세션 첫 입력으로 다음 중 하나 — 결과 동일 (SEED self-contained):
- `load` (Serena MCP session-loading skill — pinion 활성화 시 SEED + memory 자동 hydrate)
- `@docs/SEED_PROMPT.md 읽고 R<현재 라운드> 자동 진행`
- 단순 `R<현재 라운드> 진행`

세션 진입 시: (1) 위 R689 land 블록 + 다음 라운드 plan + watch-out + lessons 읽고, (2) 사용자가 "교과서적/SSOT/북극성?" 감사 요청하면 `[[verify-seed-claims-audit-first]]` 대로 grep+read 독립 감사 → smell 발견 시 feature 전 inline 청산 라운드부터, (3) 그렇지 않으면 R690 atomic plan 첫 atomic 부터 자동 진행. 이전 commit/변경은 git log + GENERATED.md 가 source of truth.

【진입 시 필독 순서】
1. `docs/SEED_PROMPT.md` (이 파일 — single-command entry point)
2. `docs/GENERATED.md` §1 Vision (4-phase) + §2 invariants + §3 capability boundaries + §5.15 8-point contract + 최근 R<NNN> changelog 엔트리
3. `mnemosyne://concepts/overview` + anti-patterns + atomic-store + frozen-ledger
4. `CLAUDE.md` (Project identity + Hard invariants + 4-phase 표)
5. `~/.claude/CLAUDE.md` + `COMMIT_FORMAT.md` (72-byte 줄 / Co-Authored-By 금지 / English-only commit)
6. `git log --oneline -30`
7. `memory/MEMORY.md` — 특히:
   - `[[true-north-star-phases]]` ★ 가장 중요
   - `[[project_scope_game_engine]]` / `[[textbook-long-term-correct]]`
   - `[[verify-seed-claims-audit-first]]` ★ ("교과서적?" 질문 = grep+read 감사)
   - `[[round-direction-auto-select-no-ask]]` (방향 묻지 말고 자동 진행)
   - `[[abstraction-needs-second-consumer]]` / `[[substrate-incompleteness-signal]]`
   - `[[r47-class-incident-prevention]]` / `[[native-perf-gap-philosophy]]`
   - `[[multi-external-substrate-extra-externals-pattern]]` / `[[ai-first-rpc-introspection-obligation]]`
   - `[[sce-priority-over-pinion]]` / `[[sce-upstream-debts]]` (SCE-004)
   - `[[owner-cache-typed-key]]` / `[[owner-cache-no-nested-factory]]`

【R700+ 로드맵 (Phase B/C/D)】

- Phase B (Professional GUI, ~25%) — multi-window substrate ✓ (R670.B); DevTools/Inspector ✓ (R675-R683); dock/splitter/editor ✓ (R685-R688). **남은 것**: 위젯 카탈로그 폭 (Menu / Dialog / Toolbar / Table / RichText / Tabs(R690) / Tooltip / Drawer / Accordion / DatePicker / ColorPicker), Model/View + 대용량 virtualization, OS 네이티브 통합 (file dialog / native menu / drag-drop / print), API 안정화. **플랫폼(Eclipse-급) 확장 모델 결정** = make-or-break spec round 후보 (Rust 안정 ABI 부재 → WASM component / scripting / 재컴파일 확장점 중 택1; API 형태 좌우하므로 early 결정).
- Phase C (Game engine substrate, ~12%) — R1000+. immediate-mode canvas ✓ (R681) + dirty cache ✓ (R682). **남은 것**: `ImmediateModeNode` primitive + game-loop (60-144fps lockstep + delta + frame budget) + per-`Scene::Container` retained↔immediate runtime switch + 3D scene graph + asset pipeline + physics + audio + gamepad + PBR (전부 0%).
- Phase D (AAA game maker, 0%) — R2500+. Unreal-class editor self-hosted in pinion + visual scripting + Nanite/Lumen-class rendering + multiplayer netcode. dock editor 가 씨앗.

【각 라운드 의무】
1. **visible deliverable 의무**: 매 라운드 cargo run + demo script (process/audit-clearance 라운드 제외)
2. **RPC verify demo 의무**: ≥ 30 assertion (R660 baseline)
3. **inline 부채 청산 mandatory**: 이전 라운드 honest 약점 → 다음 라운드 mandatory 인라인 청산. 외부 의존만 carry 정당
4. **doc compression baseline (R661)**: target ≤ 1.5x base LOC; 압축 density 유지
5. **검증 3종**: `cargo test --workspace` + `cargo clippy --workspace --all-targets --features pinion-runtime/vello` (`-D pedantic`) + 전체 demo sweep (현재 46개, R690 후 47개). 모두 green 일 때만 commit
6. **Mnemosyne**: validate_workspace baseline → append_changelog_entry_v2 R<NNN> → validate_workspace 재확인 (T1 reject=0 / round-trip 1/1 / 새 orphan +0). atomic JSON / GENERATED.md 직접 편집 금지
7. **2-commit 패턴**: 코드 commit (`refactor/feat(scope): R<NNN> ...` + atomic JSON + GENERATED.md) → meta commit (`docs(meta): R<NNN> SEED record <hash> + R<next> next`, SEED 만)

【watch out — active carry (cleared ✓ 항목은 git/GENERATED 참조)】

영구 carry (외부 의존 — 우리가 못 푸는 것):
- SCE-004 (Forge codegen serde derive) — vendor/sce upstream RFC; SCE-002 (consumer-injectable derive list) 같은 axis
- vello/wgpu 환경 의존 (headless PINION_SCREENSHOT); Figma API token
- pinion-tui multi-window 미지원 (terminal 1 process = 1 alternate-screen 본질)
- multi-window animation tick share (one tick per ShellCore-frame) — per-spec timing 요구 시 별도 axis
- 모바일(iOS/Android) 빌드 0% — 스택 이론 호환, device measurement 까지 deferred ([[mobile-target-deferred]])

R687 carry (Phase B/D editor candidates):
- live MOUSE drag-to-reorganize + drop-zone highlight overlay — 현재 RPC-native only; shell drag-session 이 `resolve_dock_drop` in-process consumer 로 layer (R688 reconcile + R689 gate 정리로 dynamic external 재등록 깨끗)
- DockReorganizeIntent undo/redo — immutable `Result` form 이 undo-stack 친화; Phase D workspace history

R690 carry (deferred — evidence-first):
- reconcile generation-counter dirty-gate (editor 도 idle frame skip) — Phase D editor 가 동적 split 다수 생성 + perf signal 시 (evidence-first, 현재 1 small binding → premature ×)
- Scene Clone via External handle `Box<dyn>` → `Rc<RefCell<dyn>>` (S15 / R684.B Hack 3.4) — Phase C entry 자연 land (immediate-mode game-loop + dirty cache 도 이득)

cross-widget SSOT 부채 (R696.A 노출):
- `widget_state_name!` adoption sweep — checkbox/radio/button/slider 의 External introspect query/invoke 가 아직 local `*_state_name` fn 사용(R643 primitive 가 retire 하려던 것; 미채택). State::as_name() 로 라우팅 = behaviour-preserving, low-cost cleanup 라운드. Disclosure(R696.A)가 유일하게 완전 라우팅.
R697 carry (accordion 잔여):
- Accordion single-open(exclusive) variant — framework-owned single-expand coordination(RadioGroup mould); 2nd consumer/명시 필요 시 land. 현재 multi-open(APG 기본)만.
- accordion 헤더 visual focus-ring 無 — R694 composite-paint focus-ring carry 상속(view_disclosure ring 미그림). focus 는 AT-reported + RPC-verifiable; 시각 표시는 focus-ring substrate 라운드 일괄.
R696 carry (disclosure/aria-expanded 잔여):
- aria-expanded 2nd/3rd consumer latent: submenu title(menu.rs aria-haspopup/expanded, 현재 structural) + tree-row twisty(TreeItem 현재 glyph-only) — 그 라운드에서 `AccessNode.with_expanded` retrofit.
- disclosure content height-collapse 애니메이션 — 현재 instant show/hide; spring height 는 measured-content clip substrate 필요(Phase B polish; accordion 도 공유).
- 선택적 role=region on panel — labelled-landmark 2nd consumer 등장 시 `AriaRole::Region` 추가(현재 APG-optional, [[abstraction-needs-second-consumer]]).
R695 carry (tooltip 잔여 + cross-widget 부채):
- M3 plain-tooltip inverse-surface 팔레트(`inverseSurface`/`inverseOnSurface` ColorRole) 부재 — 현재 `SurfaceContainerHighest`(valid M3 rich-tooltip tone) 사용; inverse roles 는 별도 palette-completeness axis(R590 error-tier 선례), tooltip 위젯 부채 아님.
- generic "임의 위젯에 tooltip 부착" anchor→overlay 결합 primitive — hello-tooltip 의 anchor 가 곧 TooltipExternal; 일반 back-channel 은 2nd consumer 대기 [[abstraction-needs-second-consumer]].
- M3 4dp tooltip offset — shared-tag hoverable contiguity 위해 gap 0 필수; 가시적 gap 은 transparent hit-bridge primitive 필요.
R694 잔여 (focus-ring 부분 부채, 여전히 open):
- (a) standalone #[widget]-derive 버튼(hello-button/figma) ring 無 — derive macro 가 `focused` slot 을 State 에 surface 해야(derive-macro axis; a11y 는 focus 정확 보고). (b) Tabs(R690) automatic-activation = focus=selection(Accent indicator 겸함), 별도 keyboard ring + manual-activation state-layer = 별개 axis(tabs.rs). (c) M3 focus-ring offset gap(2dp) = border-offset primitive 부재.
- Dialog 추가 axis: light-dismiss (backdrop click=close; scrim external 화), M3 elevation shadow (shadow primitive 부재), scrollable content / icon / divider panel slot, nested modal stacking (substrate stack 지원 — consumer 無).
- **roving-tabindex command container** 2-consumer (MenuBar + Toolbar); 3rd 시 Rule-of-Three lift.
- Menu 잔여 (R691): click-outside dismiss = overlay-dismiss substrate (아래 cascade), content-anchored dropdown, aria-haspopup/expanded, submenu, accelerator, dropdown shadow.

Phase B widget catalog cascade (R697+):
- R690 Tabs ✓ / R691 Menu ✓ / R692 Toolbar ✓ / R693 Dialog ✓ / R694 focus-ring substrate ✓ / R695 Tooltip ✓ / R696 Disclosure ✓ / R697 Accordion ✓ (multi-open APG; Disclosure substrate 2nd consumer via create_extra_externals + per-header Tab stops + arrow-roving focus; 신규 core widget 0)
- R698 = `widget_state_name!` adoption sweep (R696.A carry, low-cost cleanup) / Table (Model/View, multi-round — virtualization substrate-first) / Drawer (anchored edge slide + overlay-dismiss substrate-first) / Accordion single-open variant / DatePicker / ColorPicker. 진입 시 class(command/selection/descriptive/container) + substrate 전제 먼저 감사
- TreeView 확장: multi-select / drag-drop 재정렬 / virtualization (R750+); generic `TreeRowRouterExternal` lift (2nd consumer 시)
- **overlay-dismiss substrate** (cross-widget: menu click-outside + popover + combobox + Drawer): click-outside 2nd consumer(Drawer/Popover) 등장 시 menu click-outside 와 일괄 설계 (Tooltip 은 pointer-leave/blur dismiss 라 click-outside consumer 아님; 지금 1 consumer → premature, [[abstraction-needs-second-consumer]])

【lessons — 누적】
- **Vision spec 명시 = 모든 axis 선택 anchor** (R663.5). axis 선택 시 매번 self-check: 이 라운드가 진짜 northern-star (4-phase 종착)에 얼마나 가까이 가는가?
- **Substrate-first ordering 정통** — framework-first primitive → consumer round. [[r47-class-incident-prevention]]
- **Mirror-substrate / mirror-migration** — 비슷한 시스템은 canonical reference 1곳 + byte-level mirror; 새 helper 추출은 N≥6 까지 미룸 ([[abstraction-needs-second-consumer]] / Rule of Three)
- **Substrate gap 청산 시 application override audit 의무** — copy-pasted scaffolding 이 gap 뒤에 latent bug 숨김
- **Effect-retention** — production Effect handle 영구 retain mandatory (Owner::cleanup queue 가 Weak 만 보관)
- **Owner::cache nested factory 금지** — pre-resolution + framework guard ([[owner-cache-no-nested-factory]])
- **verify-seed-claims-audit-first** — SEED 의 "smell-free / documented tradeoff" 자평조차 grep+read 독립 감사로 뒤집힘 (R686.A, R687.A, R688.A, R689). "documented" 가 carry 정당화 ×; 외부 의존만 carry 적격
- **spec↔code drift 도 audit 대상** (R690.A) — spec-phase 설계(R32 §5.27 VirtualList 등)가 구현 없이 "implemented-sounding" 으로 굳으면 SSOT drift. "정말 구현됐나?" 는 grep 로 검증; 미구현이면 section field 정정 + caveat 청산 (frozen changelog body 무손상, R38 §5.22 선례). 사용자 질문이 drift trigger 가 되기도 함
- **SEED 의 plan 권유 자체도 audit 대상** (R691) — SEED 가 "기존 X substrate 재사용" 을 권해도 grep+read 로 X 실재 확인. R691 의 "hello-popover popover-anchoring 재사용" 권유는 misnomer 였음 (hello-popover = IntrinsicAfterFirstPaint window-sizing demo, dropdown-anchoring substrate 아님). 부재 substrate 는 기존 primitive (`absolute_position` overlay) 로 우회 — 없는 substrate 날조 ×
- **새 위젯 = class 먼저 분류** (R690/R691/R692) — WAI-ARIA role 기준: selection(`tab`/`option`=`aria-selected`) → RadioGroupExternal 재사용; command(`menuitem`=무상태) → 신규 External; toggle(`button`+`aria-pressed`) → `AccessState.checked`가 accesskit `set_toggled`로 lower 하므로 **a11y axis 0**, aria-pressed(button) ≠ aria-checked(checkbox)는 role 로 구분; container(`toolbar`/`menubar`=focus-only roving); descriptive(`tooltip`=무상호작용). 분류가 substrate 재사용/신규 + 신규 a11y axis 필요 여부를 결정
- **substrate 의존이 widget 선택을 좌우** (R692) — 후보 위젯의 substrate 전제를 진입 시 grep 으로 먼저 감사. modal Dialog 는 focus-trap substrate 필요한데 codebase 가 명시적 deferred (`focus_request.rs:42-50`) + `Tab` 이 `handle_focus_traverse` hardwire(apply_key swallow 불가) → half-modal=non-textbook 이므로 Dialog 연기, substrate-없이 완결되는 Toolbar 선택. 큰 substrate 필요하면 그 substrate 라운드를 먼저 (substrate-first ordering). round-auto-select 는 evidence 로 재선택 가능 (Dialog→Toolbar pivot)
- **edge-triggered widget 부작용은 reducer/External 에서, view-time Effect 금지** (R693) — modal scope open/close 같은 1-shot 부작용은 dispatch handler(reducer 또는 External::invoke)에서 mailbox 에 write 해야 `handle_tail` 의 drain 이 같은 frame 에 잡음(focus_request 와 동일 타이밍 보장). view-fn `Effect` 로 signal 감시 시 Effect 는 paint-time 에 돌아 drain **후** 라 1-frame lag + drain miss. 그래서 hello-dialog 는 Signal 을 reducer 에서 flip + modal_scope_request 도 reducer 에서 호출(둘 다 owner-wrapped + handle_tail 내부)
- **dynamic-focusable = modal scope 의 정통 해법** (R693) — `focusable_tags` 는 boot-time static 이라 "열렸을 때만 focusable" 위젯(dialog 컨트롤, todomvc 인라인 editor)을 표현 못 함(phantom tab stop). modal scope members 가 열려있는 동안만 active focusable enumeration 이 되는 설계가 그 dynamic-focusable gap 의 2nd-consumer 해법. RPC `focus/set`+`focus/get` 도 `active_tab_order` 로 modal-aware (base `tab_order` 검사하면 trap member reject + invoker no-op accept = 버그)
- **single-External 헬퍼는 multi-External(Container) scene 에서 침묵 실패** (R693) — `apply_aria_activate` 가 `let Scene::External(node)=scene else return false` 라 Container 루트(create_extra_externals 다수 위젯)에선 모든 tag 에 false. 단일-위젯 example 만 있던 헬퍼를 multi-External example 이 처음 쓰면 grep 로 전제 확인; 없으면 demo 의 keyboard-activate 경로가 잡아냄(unit test 는 단일 External fixture 라 통과해버림 — E2E demo 가 gap 노출)
- **focus posture = hover/pressed 와 동일 채널** (R694) — shell-focus 를 paint 에 전달하는 textbook 경로는 view-fn signature ripple(`focused: Option<&str>` 추가, ~30 example)이 **아니라** External posture(`on_focus_change`→`focused` introspect slot→`read_state`→paint arg)다. hover/pressed 가 이미 이 채널을 쓰므로 focus 도 동일하게 흐르는 게 일관적이고 덜 침습적(focus-ring consumer 만 touch, 전체 view-fn 불변). shell 이 이미 `notify_focus_change`→`on_focus_change` 를 fire → substrate 는 External posture 저장 + slot + ring paint 만. **substrate-dependency 가 widget 선택을 좌우(재확인)**: SEED 가 Tooltip 권장했으나 진입 감사로 textbook Tooltip 이 keyboard-focus 트리거(WCAG 1.4.13) 필요 = 미해결 focus-paint substrate 의존 → R692 Dialog→Toolbar 와 동형 pivot(substrate 먼저 R694, 의존 widget 나중 R695). round-auto-select 는 evidence 로 SEED 권장도 재선택([[round-direction-auto-select-no-ask]])
- **descriptive-class 위젯 = anchor 가 곧 위젯이면 anchor→overlay back-channel 불필요** (R695) — tooltip 의 trigger event(hover/focus/dismiss)가 위젯에 직접 도달하면 가시성 statechart 가 자기완결(`visible=(hovered||focused)&&!dismissed`). dismiss latch 는 trigger-episode 하강엣지(focus 없는 leave/hover 없는 blur)에서 reset. "임의 위젯에 tooltip 부착"하는 generic anchor→overlay 결합은 첫 round 에 짓지 말고 2nd consumer 까지 defer([[abstraction-needs-second-consumer]]) — SEED 가 "anchor→overlay 신규 패턴 필요"라 했으나 anchor=위젯 설계로 회피됨(SEED plan 도 audit 대상, R691 lesson 재확인)
- **WCAG 1.4.13 hoverable = shared-tag(`#pop` composite sub-index) 연속성** (R695) — overlay 가 anchor tag 의 `#pop` sub-tag 면 hover 라우터가 body hover 를 같은 external 로 보냄(hover-target transition 無) → timer 없이 "tooltip 위로 커서 이동해도 안 사라짐". gap 0 contiguous 위치 필수(가시 gap 은 hit-bridge primitive 필요). 동시에 overlay 는 독립 paint tag 라 `scene/bbox`/snapshot 으로 위치 조회 가능 = composite-tag 가 hoverable + addressing 둘 다 해결
- **Escape 는 widget-first + quit fallback 이 정통** (R695) — 기존 modal-only Escape 특수처리(`focus_is_modal` 이면 apply_key, 아니면 `event_loop.exit`)를 일반화: 항상 focused widget `apply_key`(`try_apply_key`)에 먼저 위임, 미처리 + 비modal 시에만 quit. 다른 모든 key 가 widget-first 인 모델과 일관 + Tooltip dismiss(WCAG)·Dialog cancel 단일 funnel. 기존 widget 들은 Escape 미처리(false) → quit 보존(behavior 무변). `scene/key "Escape"` RPC 는 `handle_named_key`→`try_apply_key` 경로(winit Escape arc 와 별개)라 demo 로 keyboard funnel 검증 가능
- **additive a11y 축은 AccessNode 필드, AccessState 아님** (R696) — 새 ARIA 상태(aria-expanded)를 AccessState 에 넣으면 enum-field 가 아니라 struct literal 이라 ~20개 hand-written `AccessState{..}`(a11y_manual 바인딩 — todomvc/settings/listbox/radio_group/dialog/menu/tabs/toolbar/slider/textfield...)가 전부 새 필드 enumerate 강제(컴파일 break). 정통 layer 는 AccessNode 필드 + `with_X` builder + `new()` 기본값 — selected(R51.98)/modal(R693)/described_by(R695)/level·posinset·setsize(R674) 가 전부 그렇게 들어와 리터럴 무손상. (AccessState 의 5개 interaction flag 는 초기부터 있던 historical set; checked 만 거기 있는 이유.) derive 도 AccessState literal 이 아니라 `.with_X(..)` chain(value_chain 동형)으로 emit. **workspace test 가 layer 오판을 잡음** — AccessState 에 먼저 넣었다가 todomvc literal break 로 즉시 정정; substrate layer 결정은 1st consumer 만으로 부족하고 "기존 consumer 전수"를 cargo test 로 확인해야 함
- **mirror 는 정통 부채까지 복제한다 — SSOT primitive 존재 여부를 mirror 전에 grep** (R696.A) — checkbox 를 byte-mirror 하면서 checkbox 의 pre-R643 state-name 부채(local `*_state_name` fn + 예제 `parse_*`)까지 3중 복제로 가져옴. R643 `widget_state_name!`(as_name + from_name_or_default 를 변종 1리스트로 emit)가 이미 그 부채를 retire 하려고 만들어졌는데 mirror 소스(checkbox)가 미채택이라 안 보였음. **mirror 정통성은 reference 의 정통성에 종속** — mirror 전 "이 매핑/보일러플레이트에 SSOT primitive 가 이미 있나?" grep 必. 사용자 "교과서/SSOT?" 리뷰가 트리거가 됨([[verify-seed-claims-audit-first]] — 자평 green 도 독립 감사로 뒤집힘)
- **작은 substrate 는 1st consumer 와 함께, 큰 substrate 는 substrate-first** (R696) — aria-expanded 축 + derive 확장은 작아서 Disclosure 위젯과 한 라운드 land(aria-describedby+Tooltip R695, aria-selected+listbox 선례). 반대로 Table 의 virtualization, Drawer 의 overlay-dismiss 는 큰 substrate라 그 substrate 라운드를 먼저(R694 focus-ring→R695 Tooltip 패턴). 진입 grep 으로 후보의 substrate 전제 크기를 재서 분기 ([[substrate-incompleteness-signal]] / substrate-dependency-드리븐 위젯선택)
- **불완전 grep 결론은 dedicated-symbol grep 으로 재확인** (R694) — `crates/pinion-shell/src/` 광역 grep 이 `on_focus_change` 미검출 → "GUI 가 focus 를 External 에 안 알린다" 오판 → substrate 설계 시작 직전 `notify_focus_change` 직접 grep 으로 ~9 site fire 확인하여 정정(redundant shell wiring 작성 회피). audit-first 가 코드 작성 전에 false premise 를 잡음([[verify-seed-claims-audit-first]])
- **환경 메모리 cap** — full `cargo test/build --workspace` (default -j = all cores) 가 동시 링크 스파이크로 세션 OOM-kill (스왑 압박 환경). `-j2`(CARGO_BUILD_JOBS=2) 로 cap; commit 시 hook clippy 도 `CARGO_BUILD_JOBS=2 git commit` 으로 전파
- **composition widget = substrate 의 2nd-consumer 검증 라운드; APG 기본값을 정통 base 로** (R697) — Accordion 은 새 statechart 없이 N Disclosure 를 `create_extra_externals` 로 합성한 consumer round([[substrate-incompleteness-signal]] 검증 — boilerplate 5+ LOC 없이 깨끗이 합성되면 substrate 건강). 변종(single-open exclusive)이 framework 조정을 요구하면 그건 RadioGroup-식 mutual-exclusion substrate 라 1st-consumer 에 안 넣고 defer([[abstraction-needs-second-consumer]]); WAI-ARIA APG 가 명시한 **기본값(accordion=multi-open)**을 base 로 삼아 신규 substrate 0 으로 land. accordion-vs-"Disclosure 3개 쌓기"의 실질 차이 = container keyboard model: `focusable_tags()` 가 헤더 전부(각자 Tab stop, RadioGroup single-tab-stop roving 과 대조) + arrow-roving 은 `focus_request` mailbox 로 focus 만 이동(expand 안 건드림). substrate 검증·키보드 모델·a11y(flat Button+aria-expanded, accordion role 無)가 라운드의 실체
- **demo storage isolation 의무** — persistence axis 등장 시 기존 demos 의 isolation pattern audit (`isolated_storage_dir`)
- **SEED 비대화 주의** — SEED 가 ~100K 토큰 넘으면 `/load` 시 auto-compaction → thinking-block 손상 → API 400 (`/load` 멈춤; Claude Code 미해결 버그 #13012/#12973). 과거 land 기록은 git+GENERATED 가 SoT 이므로 SEED 엔 최근 1라운드 + 영구 섹션만 유지. 라운드마다 직전 블록 prepend 시 그 이전 블록은 제거 (2026-05-28 1095→111줄 정리)

【명시적 금지】
- README.md / docs.rs / user guide proactive 생성 금지
- Material Symbols / 외부 폰트 vendor commit 금지
- macro magic / 숨겨진 동작 channel 금지
- vendor/sce 직접 수정 금지 (SCE-004 등록 후 PR 경로만 정통)
- process round (0 LOC code change) 연속 2 이상 금지
- visible deliverable 없는 라운드 금지 (process maturity / audit-clearance 라운드는 예외)
- doc-heavy LOC 정당화 자동 허용 금지 — R661 baseline 유지
- Effect handle drop (production code) 금지 — Owner::cleanup queue 가 Weak 만 보관
- §1 vision 추가 권유 / 에셋·물리·오디오 spec add 권유는 Phase C 진입(R1000+) 전까지 금지 ([[project_scope_game_engine]])
- Persistence schema breaking-change 시 PERSISTED_SCHEMA_VERSION bump 누락 금지
- commit message: English-only / 72-byte 줄 / Co-Authored-By·Generated-with 태그 금지 (COMMIT_FORMAT.md; commit-msg hook 강제)

【프롬프트 사용법】
새 세션 첫 입력으로 `load` (또는 `@docs/SEED_PROMPT.md 읽고 진행`). 이 SEED 는 self-contained: 불변 운영 원칙 + 최근 1라운드 land + 다음 라운드 plan + watch-out + lessons + 금지. 과거 라운드 상세는 git log + GENERATED.md. **SEED 는 슬림 유지** — 라운드 close 시 직전 land 블록을 새 것으로 교체(누적 ×), watch-out 의 cleared 항목 제거.
