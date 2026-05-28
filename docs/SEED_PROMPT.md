# pinion seed prompt — 매 세션 첫 입력

> **R693 land (2026-05-29, commit `d089de7`)** — Phase B widget-catalog 계속; R692 가 substrate-first 로 연기한 **modal-focus-trap 부채 청산 + modal Dialog widget**. 진입 감사로 R692 SEED 주장 재확인 (`[[verify-seed-claims-audit-first]]`): `focus_request.rs:46-50` 가 "focus traps for modal dialogs" 를 명시적 deferred axis 로 선언 + `app.rs:899` Tab=`handle_focus_traverse` hardwire(apply_key swallow 불가) → substrate-first 정당. **핵심 설계**: (1) `FocusManager` modal scope (`pinion-runtime/focus.rs`) — `Vec<ModalScope>` **stack**; `push_modal_scope(members)`=현재 focus 저장 → members 가 **active focusable enumeration** 이 됨(static `focusable_tags`/`tab_order` 에 **일부러 부재** = dynamic-focusable; todomvc R664 phantom-tab-stop gap 의 2nd-consumer 해법) → 첫 member auto-focus; `pop_modal_scope`=invoker 복원. nested modal 지원(stack). (2) `pinion-core::modal_scope_request` mailbox (focus_request mirror) — `Open{members}`/`Close` 를 **reducer/External 에서 write**, **두 backend(shell+tui) `handle_tail` 에서 drain** → push/pop. reducer 가 handle_tail 안에서 돌아 drain **직전** 실행 → Effect 불필요(view-time Effect 면 drain ordering 어긋남). (3) **Escape**: modal 활성 시 양 shell 이 Escape 를 exit/quit 대신 widget `apply_key` 로 라우팅(WAI-ARIA "Escape closes dialog not app"). (4) a11y: `AriaRole::Dialog`(accesskit `Role::Dialog`) + `AccessNode.modal`→accesskit `set_modal`(aria-modal). (5) RPC `focus/set`+`focus/get` modal-aware(`active_tab_order`) → AI client 도 trap confinement. (6) `pinion-widget-paint::dialog`(신규) — `view_dialog` 전창 scrim(topmost hit-test 로 배경 click 차단; scrim tag 에 external 無 → swallow = no light-dismiss = textbook modal) + 중앙 M3 panel + `DialogStyle`/`DialogContent`. (7) `examples/hello-dialog`(신규 첫 consumer) — destructive-confirm; trigger(primary)+OK/Cancel(extra) **실제 Button external 3개**; reducer 가 `dialog_open`/`dialog_result` Signal flip + modal_scope_request; 단방향 흐름(reducer→widget back-channel 無). (8) `tools/demos/r693_dialog.py` (41 assertion). **honest gap (carry)**: action 버튼 focus **RING 미paint** — `ButtonState` 에 focus posture 없고 view-fn 이 shell focus 못 봄(R690 Tabs/R692 Toolbar 와 동일 shell-focus-paint axis); trap 자체는 real + RPC(`focus/get`)+a11y(`aria-modal`)로 관측. `apply_aria_activate` 가 single-External 전제 → hello-dialog 가 첫 multi-External button-activate consumer 라 Container-descending 변종 inline(2nd consumer 시 substrate lift). **검증**: `cargo test --workspace` 60 bin 0-fail(+33 신규: focus modal 11 / modal_scope_request 5 / dialog paint 7 / a11y 2 / hello-dialog 9), clippy `-D pedantic` clean(`view_dialog` 9→7 arg = `DialogContent` struct + `viewport` tuple 그룹), demo r693 PASS + regression(toolbar/menu/tabs) PASS, Mnemosyne `R693`(ledger 569→570; T1 new=0; RT 1/1; GENERATED sync; impact_refs [5.16,5.39,5.40,5.41,5.50]). **환경 주의**: full `cargo test/build --workspace` `-j2`(CARGO_BUILD_JOBS=2) 필수(OOM); commit 시 hook clippy 도 `CARGO_BUILD_JOBS=2 git commit` 전파. **R693.A (commit `cfa89fb`) 자가-감사 청산** (사용자 "hack/smell 없이 교과서적?" 감사 → grep+read 독립 검증 → 2 smell 청산): (1) hello-dialog a11y names hardcode = **SSOT 위반** (`enrich_names_from_scene` 는 None 만 채움 → hardcode 는 paint label 과 drift 하는 평행 source) → `with_name` 제거, paint `TextNode` 가 SSOT (hello-button/hello-menu 선례); (2) `apply_aria_activate` 가 `scene` 를 `Scene::External` 로 가정 → multi-External Container root 에서 **모든 tag 침묵 실패** → `find_external_with_tag_mut` descend 로 substrate 수정(single+multi 양쪽 serve), example 의 inline 15-LOC dup 제거 ([[substrate-incompleteness-signal]]: 첫 client boilerplate = 즉시 substrate 수정). 남은 정직 gap = action 버튼 focus **RING** (Button focus posture 부재).
>
> **다음 세션 진입**: `load` 단독 입력. R694 = Phase B widget catalog 계속. 후보: **(a) Tooltip** (descriptive-class; **2nd 진짜 overlay-dismiss consumer** → menu click-outside + tooltip + popover + combobox 를 묶는 overlay-dismiss substrate + anchored positioning 설계 trigger; substrate-first 高leverage); **(b) Table** (Model/View grid — DCC/IDE 핵심 데이터 위젯; 대형 → multi-round slice); **(c) Drawer/Accordion** (1-round each, container-class). 진입 시 위젯 class(command/selection/descriptive/container) 먼저 감사. **권장 (a) Tooltip** — overlay-dismiss substrate 가 Menu(R691)부터 미뤄온 cross-widget 부채이고 Tooltip 이 그 2nd consumer 트리거. 단 Dialog 는 modal(no dismiss-on-outside)이라 overlay-dismiss 의 1st consumer 가 아님에 유의.
>
> R693 가중 진척: Phase A 97% + Phase B 25% × ~90% + Phase C 35% × ~12% = 북극성 가중 **~38%** (Dialog = catalog +1 widget + modal-focus-trap substrate; 가중 微).

> **이전 라운드 land 기록 (R1 ~ R688.A)**: `git log --oneline` + `docs/GENERATED.md` (Mnemosyne 렌더 changelog) 이 single source of truth. 이번 SEED 정리(2026-05-28, R689 세션 후속)에서 비대화(`/load` 시 auto-compaction → thinking-block 손상 → API 400 유발)를 막기 위해 과거 land 블록 + DONE plan 절 + 직전-N-세션 상세 기록을 SEED 에서 **제거** — 전부 git 히스토리 + GENERATED.md 에 무손실 보존됨. 특정 라운드 상세가 필요하면 `git show <hash>:docs/SEED_PROMPT.md` 또는 `git log -S"<키워드>" docs/SEED_PROMPT.md` 로 조회.

【불변 운영 원칙】 (매 세션 동일)
- 비용 무시. 항상 장기적으로 올바른 textbook-canonical 선택
- **진짜 북극성 = AAA game shippable + Unreal-class editor self-hosted in pinion itself, AI-introspection 1st-class through every phase.** 4-phase progression: A. Foundation (현재 ~97%) → B. Professional GUI (Qt/Flutter/Compose-class + multi-window + DCC widget catalog, R700+) → C. Game engine substrate (§2 #4 immediate-mode game loop ↔ retained widget tree dual + 3D + assets + physics + audio + PBR, R1000+) → D. AAA game maker (editor self-hosted + visual scripting + Nanite/Lumen-class rendering + multiplayer netcode, R2500+). 현재 가중 진척 ~36-38%. R667 = Phase A 종료 = **NOT 북극성 도달**
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

R693 carry (Dialog deferred axes + cross-widget 부채):
- **shell-focus-paint axis** (R690 Tabs / R692 Toolbar / R693 Dialog 누적) — view-fn 이 shell `FocusManager` focus 를 못 받음 + `ButtonState` 에 focus posture 無 → multi-tab-stop 위젯의 keyboard focus RING 미paint. Dialog action 버튼이 3번째 미충족 consumer. 해법 후보: view-fn 에 `focused: Option<&str>` 추가(framework-wide signature ripple, ~30 example) 또는 `on_focus_change` 가 tag/sub-tag 운반(External trait ripple). trap/roving 자체는 RPC+a11y 로 관측되므로 visible-but-no-ring 은 accepted carry. 3-consumer 누적 = lift 임박 signal.
- ~~apply_aria_activate single-External 전제~~ ✓ CLEARED R693.A (find_external_with_tag_mut descend; single+multi 양쪽 serve).
- Dialog 추가 axis: light-dismiss (backdrop click=close; scrim 을 external 화), M3 elevation shadow (shadow primitive 부재), scrollable content / icon / divider panel slot, nested modal stacking (substrate stack 이미 지원 — consumer 無).
- **roving-tabindex command container** 2-consumer (MenuBar + Toolbar); 3rd 시 Rule-of-Three lift.
- Menu 잔여 (R691): click-outside dismiss = overlay-dismiss substrate (아래 cascade), content-anchored dropdown, aria-haspopup/expanded, submenu, accelerator, dropdown shadow.

Phase B widget catalog cascade (R694+):
- R690 = Tabs ✓ (selection-class, RadioGroupExternal 재사용)
- R691 = Menu ✓ (command-class, 신규 MenuBarExternal)
- R692 = Toolbar ✓ (command+toggle container, 신규 ToolbarExternal; a11y axis 0)
- R693 = Dialog ✓ (modal-focus-trap substrate + scrim chrome; FocusManager modal scope + modal_scope_request mailbox)
- R694+ = Tooltip (descriptive; overlay-dismiss substrate 2nd consumer) / Table (Model/View, multi-round) / Drawer / Accordion / DatePicker / ColorPicker (1라운드 1위젯 또는 small pair). 진입 시 class(command/selection/descriptive/container) 먼저 감사
- TreeView 확장: multi-select / drag-drop 재정렬 / virtualization (R750+); generic `TreeRowRouterExternal` lift (2nd consumer 시)
- **overlay-dismiss substrate** (cross-widget: menu click-outside + tooltip + popover + combobox): 2nd overlay consumer 등장 시 일괄 설계 (지금 1 consumer → premature, [[abstraction-needs-second-consumer]])

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
- **환경 메모리 cap** — full `cargo test/build --workspace` (default -j = all cores) 가 동시 링크 스파이크로 세션 OOM-kill (스왑 압박 환경). `-j2`(CARGO_BUILD_JOBS=2) 로 cap; commit 시 hook clippy 도 `CARGO_BUILD_JOBS=2 git commit` 으로 전파
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
