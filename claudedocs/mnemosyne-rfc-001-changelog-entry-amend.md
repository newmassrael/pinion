# Mnemosyne RFC 001 — Changelog entry amend / supersede primitive

**Status**: **WITHDRAWN (self-reject, 2026-05-17)**
**Target repo**: `mnemosyne-mcp`
**Filing repo**: `pinion` (consumer)
**Filing date**: 2026-05-17
**Withdraw date**: 2026-05-17 (same day, before submission)
**Trigger incident**: pinion `R45` (entry `416`) decision_summary 누락 prefix

---

## WITHDRAWAL RECORD

본 RFC 의 **premise §1.1 ("changelog 만 append-only — 정정 surface 부재")** 가 사실과 다름. pinion 측 review 결과:

- mnemosyne main HEAD = `593b28c` (R301 hard reject gate 기준) 가 R294-R301 publishable setter chain 을 이미 보유.
- `AtomicChangelogEntry` 가 audit half (frozen, T2 scope) ↔ publishable half (mutable view layer) **two-layer schema** (R294 schema_version 4).
- MCP surface (R295 / R299 / R297 / R300 / R301):
  - `set_publishable_decision_summary(entry_id, new_value)` 등 publishable setter
  - `redact_term(pattern, replacement, scope, mode, reason, applied_in, dry_run)`
  - `emit_publishable_override_ledger_draft(entry_id)`
  - `commit↔ledger drift gate` (R301 hard reject) — 정정 audit 강제
- 이 chain 은 본 RFC 의 3 option (A amend / B supersede / C edit) 을 **모든 축에서 dominate**:
  - audit 손상 0 (audit half schema level 분리)
  - entry_id 소비 0 (Option B 의 +1/정정 비용 자동 해소)
  - `reason` mandatory + `content_hash` anchor (Option B 의 reason mandatory 보다 엄격)
  - GENERATED.md template 변경 0
  - 마지막-entry race 없음 (Option A 의 단점 자동 해소)

### Root cause of the mis-filing

pinion 측 (저자) 의 **MCP tool discovery 실패**. `ToolSearch` 로 "changelog edit/update/delete" 키워드만 시도하고 `publishable` / `redact` / `override_ledger` 키워드 미시도. R294-R301 surface 가 deferred tool list 에 등록되어 있었더라도 lookup 못 했을 가능성. (실제 host 환경: 본 RFC 작성 시점에 mnemosyne-mcp build 가 R294 이전 — host 측 server 재빌드 필요 별도 issue.)

### Withdrawal closure

- 본 RFC 의 §1.3 "검색 grep 깨짐 / 외부 reference / 체계 신뢰" 비용 분석 중 (a) (b) 는 audit 도구 anti-pattern 이라는 review 지적 수용. (c) 만이 별도 mini-RFC 가치 (decision_summary prefix format 을 validator T3 rule 로 승격할지) — 이건 정정 primitive 가 아닌 **append-time 예방 primitive** 영역, 본 RFC 와 별도.
- 본 RFC 는 reject-self 로 closure. 파일은 audit trail (작성 → 즉시 review → withdraw 의 timeline) 보존을 위해 본문 그대로 유지.

### Follow-up actions (pinion 측)

1. **R45 entry 416 정정**: host mnemosyne-mcp 재빌드 (main HEAD 593b28c 또는 이후) 후 `redact_term(pattern="§5.16 SceneRenderer 표현", replacement="Round 45 — §5.16 SceneRenderer 표현", scope="decision_summary", mode="literal", reason="R45 round prefix 누락 정정 — 직전 412-415 entry 와 prefix 일관성 복원", applied_in="pinion R46+", dry_run=false)` 1-call.
2. **R46 round entry** 의 `carry_forward_bullets` 에 "RFC 001 self-withdraw — 기존 publishable setter chain (R294-R301) 로 충족, R45 prefix 정정 redact_term 으로 완료" 기록.
3. **mnemosyne 측 discovery 격차 fix** 는 RFC 가 아닌 light-weight issue 로 분리 — `SCHEMA_GUIDE.md` 또는 `RECOVERY_PATTERNS.md` 에 "typo / format fix 워크플로우" 섹션, MCP tool description 에 "for typo fix on already-appended entry" use-case 명시. (선택 — pinion 측 ROI 보다 mnemosyne 측 author convention.)
4. **format invariant 승격 미니 RFC** 는 author convention 의 strict 정도에 대한 별개 정책 논의로 분리 — 본 RFC 와 무관.

---

## 아래는 withdraw 전 원본 RFC 본문 (audit 용 보존)



---

## 0. TL;DR

Mnemosyne atomic store 는 frozen-ledger 패러다임상 entry 가 immutable 인 것이 textbook (event sourcing / audit trail). 그러나 현 primitive 집합에는 **session-scoped amend** 또는 **supersede** surface 가 없어, *방금 작성한 entry 의 typo / format 불일치* 같은 정상 maintenance 영역까지 immutable 처리됨 — `remove_inventory_entry` 가 inventory 측에는 존재하는 것과 비대칭.

요청: changelog entry 영역에도 *audit 안전한* 정정 surface 1종 추가.

권장: **Option B — `supersede_changelog_entry`**. event sourcing 정통, immutability 보존, audit trail 영구 유지.

---

## 1. Context

### 1.1 현 primitive 집합 (Mnemosyne MCP, 2026-05-17 시점)

Changelog 측:
- `append_changelog_entry_v2` — append-only, `entry_id` 순증 강제

Inventory 측 (비교군):
- `add_inventory_entry`
- `remove_inventory_entry(inventory_id, reason)` — mandatory `reason` 가 audit 안전장치
- `set_inventory_status`
- `set_inventory_section_ref`

Section 측 (또 다른 비교군):
- `add_section_caveat` (append-only)
- `set_section_rationale` / `set_section_intent` / `set_section_outputs` (replace)
- `set_section_alternatives` 등 (replace)
- `add_section_implementation` / `remove_section_implementation`

**관찰**: section 과 inventory 양쪽 모두 "추가 / 갱신 / 제거" surface 가 대칭으로 존재하지만, **changelog 만 append-only** — 정정 surface 부재.

### 1.2 Trigger incident

pinion 의 R45 round entry (entry id `416`) 가 mnemosyne `append_changelog_entry_v2` 로 작성될 때 `decision_summary` 가 `"§5.16 SceneRenderer ..."` 로 시작. 직전 entries (412–415) 의 패턴 `"Round 41 — §5.16 ..."` / `"Round 44 — §5.34 ..."` 와 prefix 불일치 — `"Round 45 — "` 누락.

결과: GENERATED.md 의 4 round 연속 row 가 한 항목만 다른 형식으로 영구화됨:

```
### 412 — Round 41 — §5.16 Vello hybrid path C ratify ...
### 413 — Round 42 — §5.34 path walker nested External addressing ...
### 414 — Round 43 — §5.34 ViewBlueprint Text/Path/Image variant parity ...
### 415 — Round 44 — §5.34 DispatchIntent ↔ scene/intents dual channel 정책 spec ...
### 416 — §5.16 SceneRenderer 표현 = pinion-forge renderer kind 빌드 코드젠 ...   ← 누락
```

이 시점에 가능한 처리는 셋:

1. 그대로 두기 — audit 1건 cosmetic 불일치 수용.
2. atomic JSON 직접 편집 — Mnemosyne 정책 위반 (`NEVER edit ... the atomic JSON directly`).
3. RFC 후 primitive 추가 → 정정.

pinion 은 옵션 1 채택 (R46 carry_forward 에 본 RFC link 남김). 그러나 root cause = primitive 부재이므로 본 RFC 로 surface.

### 1.3 왜 cosmetic 만으로 그치지 않는가

format 불일치는 *이번 incident 에서는* cosmetic 으로 보이지만:

- **검색 grep 깨짐**: `grep "Round 4" docs/GENERATED.md` 으로 round 추출하는 audit 도구가 R45 누락
- **외부 reference**: 다른 entry 가 `R45` 를 인용해도 GENERATED.md 의 title 로는 round 식별 어려움
- **체계 신뢰**: ledger 의 신뢰성은 "모든 row 가 동일 schema" 라는 invariant — 1 row 의 형식 일탈도 schema 일관성 위반

`decision_summary` 의 format 은 strict schema 가 아닌 author convention 이지만, convention 일탈도 정정 surface 부재 시 영구화. inventory 의 entry 가 잘못 등록되어도 `remove_inventory_entry(reason="typo")` 로 정정 가능한 것과 대칭이 필요.

---

## 2. Problem statement

**문제**: append-only changelog 에서 *방금 작성한 entry 의 atomic field 정정* 이 불가능. 정상 maintenance (typo / format 불일치 / impact_refs 보완) 가 audit 영구손상으로 처리됨.

**아닌 것**:
- 오래된 entry 의 사후 수정 — 이건 frozen ledger 정신 위반, 별도 봉인 영역.
- decision 의 본질 변경 — supersede 패턴이 정통 (새 round 가 옛 round 결정 변경).
- bulk rewrite — 본 RFC 의 scope 아님.

**대상**:
- 마지막 entry (또는 매우 좁은 session-recent window) 의 atomic field 정정.
- 또는 event sourcing 의 정통 처리: superseded-by mark.

---

## 3. Proposal (3 surface 후보)

### Option A — `amend_pending_entry(entry_id)`

```
amend_pending_entry(
    entry_id: str,
    decision_summary: Option<str>,
    changes_bullets: Option<Vec<str>>,
    verification_bullets: Option<Vec<str>>,
    impact_refs: Option<Vec<str>>,
    carry_forward_bullets: Option<Vec<str>>,
    reason: str,  # mandatory audit
) -> Result
```

**제약**:
- `entry_id` 가 *마지막* entry 인 경우만 허용 (또는 `--allow-stale` flag 로 보호).
- `reason` mandatory — `remove_inventory_entry` 와 대칭.
- 옵션 필드: `None` 이면 기존 값 보존; `Some(_)` 이면 replace.

**장점**:
- 가장 단순; primitive 1개.
- 사용 패턴: append → validate → amend → validate → commit (정상 maintenance 흐름).
- audit log 가 receipt 로 정정 기록 보존.

**단점**:
- *immutable ledger* 가 아닌 *mutable last-row* 패턴 — frozen ledger 정신과 trade-off.
- "마지막 entry 만" 제약을 어디서 강제할지 (session vs commit boundary) 모호함.
- "amend" 후 stash/checkout 시 commit-과거 entry 가 mutable 한 시점 존재 — race.

### Option B — `supersede_changelog_entry` (권장)

```
supersede_changelog_entry(
    superseded_id: str,
    superseded_by_id: str,
    reason: str,  # mandatory audit
) -> Result
```

후속 단계 (caller 가 별도):
1. `append_changelog_entry_v2(entry_id=superseded_by_id, ...)` — 새 entry 가 본체.
2. `supersede_changelog_entry(superseded_id=old, superseded_by_id=new, reason="...")` — old 에 `superseded_by` 필드 set.

GENERATED.md 의 old entry 측 render:

```
### 416 — §5.16 SceneRenderer ...  [SUPERSEDED BY 417]
> Superseded: 2026-05-17 by entry 417. Reason: "Round 45 prefix 누락 정정".
> Original content preserved below for audit.
...
### 417 — Round 45 — §5.16 SceneRenderer ...
```

**장점**:
- **event sourcing 정통** (Kleppmann *Designing Data-Intensive Applications*, Fowler "Event Sourcing"). frozen ledger 보존, 정정은 새 event 로 표현.
- old entry 영구 보존 — audit trail 손상 0.
- mutable surface 0 — Hickey "spec-ulation" / Hyrum's Law 안전.
- inventory `remove` 와 비대칭 해소 (inventory 측에도 같은 패턴 적용 가능 — 별도 후속 RFC).

**단점**:
- entry id 가 +1 더 소모됨 (cosmetic 정정 1건당 2 row).
- GENERATED.md render 가 `superseded_by` block 추가 — template 변경 필요.
- 사용 패턴이 amend 보다 2-step.

### Option C — `edit_changelog_entry_atomic_fields(entry_id, ..., reason)`

```
edit_changelog_entry_atomic_fields(
    entry_id: str,
    decision_summary: Option<str>,
    changes_bullets: Option<Vec<str>>,
    ...
    reason: str,
) -> Result
```

Option A 와 거의 동일하지만 *어떤 entry 도* 수정 가능 (마지막 제약 없음). `reason` mandatory 가 유일한 audit 안전장치.

**장점**: 가장 expressive.

**단점**: frozen ledger 정신과 가장 큰 충돌. 과거 entry 까지 mutable — event sourcing 위반. **권장 안 함**.

---

## 4. Recommended

**Option B — `supersede_changelog_entry`**.

근거:
- frozen ledger 정신 보존 (event sourcing 정통).
- audit 영구손상 0; 정정 자체가 ledger row 로 표현.
- inventory `remove_inventory_entry` 의 비대칭 해소 패턴이 미래에 같은 형식으로 확장 가능 (`supersede_inventory_entry` 등).
- Mnemosyne 의 `decision_status` 필드 (active / superseded 등) 와 의미 정합 — section level 의 supersede 가 이미 정통이라면 entry level 도 같음.

---

## 5. Decision required

Mnemosyne maintainer 측 의사결정 항목:

1. Option B (supersede) 채택 / 반려 / 다른 option 제시 중 어느 것인가?
2. 채택 시:
   - `supersede_changelog_entry` 의 정확한 signature (위 3.B 안 확정 / 수정).
   - GENERATED.md render template 의 `[SUPERSEDED BY N]` 표기 형식.
   - `validate_workspace` 에서 superseded entry 의 T1/T3 처리 (cross-ref 어떻게 follow).
3. 반려 시:
   - frozen-ledger 정신 보존이 최우선이고 정정은 차세대 entry 의 본문에서 author convention 으로 처리하라 — 권고가 그것인가?
   - 그렇다면 본 RFC 의 1.3 검색 grep 깨짐 / 외부 reference 같은 부수 비용은 어떻게 mitigate?

---

## 6. Out of scope

- 과거 entry (마지막이 아닌 row) 의 일반 정정 — frozen ledger 정신 위반, 명시 거절.
- atomic JSON 직접 편집 surface — 정책상 항구 금지.
- entry 의 강제 삭제 — supersede 로 충분.
- atomic field 의 schema 변경 (decision_summary format 강제 등) — 별도 RFC.

---

## 7. pinion 측 임시 처리

본 RFC 가 merge / reject 될 때까지 pinion 은:

- entry 416 그대로 보존 (정책 위반 회피).
- R46 round entry 의 `carry_forward_bullets` 에 본 RFC 링크 + "entry 416 title format gap, mnemosyne RFC 001 결과 후 처리" 항목.
- 본 RFC reject 시 pinion 측 author convention 으로 결정 (R45 prefix 누락 1건 수용).
- 본 RFC accept + Option B merge 시 pinion 측 R47+ 에서 정정 entry append + supersede call.

---

## 8. 참조

- pinion R45 round entry (id 416, 2026-05-17)
- Mnemosyne MCP primitive 집합 (2026-05-17 시점)
- SCE RFC 001 — `claudedocs/sce-rfc-001-response.md` (RFC 패턴 참조)
- Fowler "Event Sourcing"; Kleppmann *DDIA* ch.11 (event log immutability)
- Hickey "Maybe Not" / "Spec-ulation" (schema 영구성 vs 정정 surface trade-off)
