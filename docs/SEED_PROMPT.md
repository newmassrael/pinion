# pinion seed prompt — 매 세션 첫 입력

> **R692 land (2026-05-28, commit `17eb286`)** — Phase B widget-catalog 계속, R691 Menu 후속. **Toolbar widget** (roving-tabindex 가로 control strip: command 버튼 + toggle 버튼). **첫 감사 (class 분류)**: toolbar = command + toggle 두 class 를 묶는 container. command(Undo/Redo)=one-shot `Button` (R691 MenuItem 동렬); toggle(Bold/Italic/Underline)=`Button`+`aria-pressed`. **핵심**: aria-pressed 는 `button` role + toggled-state 이지 checkbox 의 `aria-checked` 가 아님 → pinion `AccessState.checked` 가 이미 accesskit `set_toggled` 로 lower (tree.rs:343) → **신규 a11y axis 0** (role 만 추가). MenuBar(dropdown 여닫음, 구조적 open state)와 대조: toolbar 는 controls 가 직접 actionable, open/closed 없음 — roving cursor + per-toggle pressed bit 만. **Dialog 미선택 사유 (감사 중 발견, `[[verify-seed-claims-audit-first]]`)**: 교과서적 modal Dialog 는 focus-trap substrate 필요 — `focus_request.rs:42-50` 가 "focus traps for modal dialogs" 를 명시적 deferred axis 로 선언하고, `Tab` 은 `app.rs:899` 에서 `handle_focus_traverse` 로 hardwire → `apply_key` 가 swallow 불가. modal auto-focus-on-open / Tab-trap / restore-on-close 는 dual-backend(GUI+TUI) focus-manager substrate 라운드 (substrate-first). half-modal = textbook 아님 → R692 = 완결 가능한 Toolbar 선택, Dialog 는 substrate 라운드로 연기. **구현**: (1) `pinion-a11y::role` — `AriaRole::Toolbar` (accesskit `Role::Toolbar` / "toolbar"); `tree.rs add_actions_for_role` focus-only container arm (MenuBar/TabList/Tree 동렬, roving tabindex single tab stop). (2) `pinion-core::widgets::toolbar` (신규) — `Toolbar` (kinds `Vec<ToolItem{Command,Toggle}>` + parallel pressed bits + roving focus) + `ToolbarExternal` (MenuBarExternal mirror); wire `invoke("send","<i>:<PtrEvt>")` (PointerUp=focus+activate) / `invoke("key","<W3CKey>")` WAI-ARIA §3.4 (Arrow L/R wrap, Home/End, Enter/Space activate); command→`"command"` intent (payload `"<i>"`), toggle→`"toggle"` (payload `"<i>.<bool>"`) + persistent flip. (3) `pinion-widget-paint::toolbar` (신규) — `ToolbarStyle::m3_default` + `composite_item_tag {bar}#{i}` + `view_toolbar` (pressed toggle = Accent@0.20 tonal fill, roving cursor = accent focus-ring border; 가로 strip, overlay 없음). (4) `examples/hello-toolbar` (신규 첫 consumer) — B/I/U toggle + Undo/Redo command; single tab stop; a11y = Toolbar root + per-control Button(toggle=checked→aria-pressed, command=none) + posinset/setsize + composite active-descendant. (5) `tools/demos/r692_toolbar.py` — 35+ assertion (toggle/command intent 는 shell **stderr** intent log 검증; focus-ring 은 paint border 검증). **roving 시각화**: paint focus-ring 은 External `focus` 추종 (tree_view 선례, always-on); shell-focus gating 은 view-fn 이 shell focus 못 봄 → R690 Tabs 가 연기한 동일 axis. a11y 는 shell-focus aware (access_node 의 focused param). **검증**: `cargo test --workspace` 4208 pass (core 1459 / widget-paint 303 / hello-toolbar 18 / a11y +2 role), clippy `-D pedantic` clean, demo sweep **48→49 PASS** regression 0, Mnemosyne `R692` (ledger 568→569; T1 new=0; RT 1/1; GENERATED sync; impact_refs [5.16,5.38,5.40,5.50]). **환경 주의**: full `cargo test/build --workspace` (default -j) 가 동시 링크 스파이크로 세션 OOM-kill → `-j2` (CARGO_BUILD_JOBS=2) 로 cap 필수 (commit 시 hook clippy 도 `CARGO_BUILD_JOBS=2 git commit` 전파).
>
> **다음 세션 진입**: `load` 단독 입력. R693 = Phase B widget catalog 계속. 후보: **(a) modal-focus-trap substrate + Dialog** (substrate-first; focus-manager modal scope + open 시 auto-focus + Tab-trap + close 시 restore, dual-backend GUI+TUI; R692 carry 참조) — Phase B 핵심, 가장 큰 라운드; **(b) Table** (Model/View grid, 대형 → multi-round slice); **(c) Tooltip** (2nd overlay consumer → anchored positioning + overlay-dismiss substrate 동시 trigger). 진입 시 위젯 class(command/selection/descriptive/container) 먼저 감사. **권장 (a)** — R692 가 substrate-first 로 연기한 부채 청산.
>
> R692 가중 진척: Phase A 97% + Phase B 25% × ~88% + Phase C 35% × ~12% = 북극성 가중 **~38%** (R691 대비 catalog +1 widget, 미미).

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

R692 carry (Dialog substrate 부채 + Toolbar deferred axes):
- **modal-focus-trap substrate (Dialog 전제, R693 권장)** — 교과서적 modal Dialog 는 focus-manager modal scope 필요: open 시 auto-focus 첫 control + Tab-trap (focusable 을 modal subtree 로 제한) + close 시 invoker 로 restore + scrim background 입력 차단. `focus_request.rs:42-50` 가 modal focus trap 을 명시적 deferred axis 로 선언, `Tab`=`app.rs:899` hardwire `handle_focus_traverse` (apply_key swallow 불가). dual-backend(GUI+TUI). visible deliverable 위해 Dialog consumer 와 묶어 land (pure substrate 단독 라운드 = visible deliverable 없음).
- Toolbar 추가 axis: hover state-layer (focused-tag→view-fn 채널 필요 = R690 Tabs 연기 axis 동일), icon glyph, min-width button, separator/section group, overflow menu, vertical orientation (aria-orientation).
- **roving-tabindex command container** 이제 2-consumer (MenuBar bar-nav + Toolbar roving); 공유 roving substrate 는 3rd consumer 시 Rule-of-Three lift 후보 (현재 premature).
- Menu 잔여 (R691): click-outside dismiss = overlay-dismiss substrate (cascade), content-anchored dropdown, aria-haspopup/expanded, submenu, accelerator, dropdown shadow.

Phase B widget catalog cascade (R693+):
- R690 = Tabs ✓ (selection-class, RadioGroupExternal 재사용)
- R691 = Menu ✓ (command-class, 신규 MenuBarExternal)
- R692 = Toolbar ✓ (command+toggle container, 신규 ToolbarExternal; a11y axis 0)
- R693+ = Dialog (modal-focus-trap substrate 전제 — R692 carry) / Table (Model/View, multi-round) / Tooltip / Drawer / Accordion / DatePicker / ColorPicker (1라운드 1위젯 또는 small pair). 진입 시 class(command/selection/descriptive/container) 먼저 감사
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
