# Path B binding axis — MNEMO-GAP-001 follow-up (RESOLVED)

> Status: **DECISION: A — REJECTED (resolved R716, 2026-05-31).** pinion's
> `§X` citations are *provenance, not implementation claims*; Path B's
> set-equality model is incompatible with that convention, so the binding
> classes stay `severity_binding="info"` (advisory). See §1.1 for the
> verified rationale. §2–§4 below are retained as the reference workplan for
> a future deliberate Option-C effort, *if* spec↔code traceability ever
> becomes a stated goal — they are not a committed plan.
> Not Mnemosyne-managed (plain `docs/` markdown, not in the atomic store and
> outside the `validate-code-refs` `crates/`+`examples/` scan — so the `§`
> tokens in here are not validated as code citations).

## 1.1 RESOLUTION (R716, 2026-05-31)

Decision **A** adopted after an independent audit ([[verify-seed-claims-audit-first]])
of this doc's own central premise — the recommendation was not taken on trust.

**Re-measured (HEAD `d1969b0`, clean):** `validate-code-refs` →
`citation_unbound=3465 impl_unbacked=81 impl_missing=33`; top bucket
`§5.16=896`. Identical to §2.

**Premise verified directly (decisive).** `§5.16` ("GPU renderer
architecture") is cited 896× in `R<round> §<section>` *header* form by files
that plainly do not implement a GPU renderer:

| File | Cite (verbatim) | What the file is |
|---|---|---|
| `composite_tag.rs` | `R659 §5.16 §5.35 — composite-tag wire helpers` | tag-string helpers |
| `reactive/owner.rs` | `R680 §5.16 §5.28 — tick only this owner's …` | reactive owner tick |
| `widgets/checkbox.rs` | `R698 §5.16 — route the CheckboxState <-> SCXML-id mapping` | state-name routing |
| `pinion-shell/Cargo.toml` | `description = "… §5.16/§5.20/§5.35 paint-side AppShell …"` | crate manifest |

These are **provenance/blame pointers** (which round touched this; which
spec-area it relates to), **not** "this file implements §5.16." Path B's
bidirectional set-equality treats a citation *as* an implements-claim, so
mechanically binding the scan would encode ~896 false "implements GPU
renderer" assertions into the spec — the exact boilerplate-as-truth failure
Path B was supposed to prevent.

**Conclusion.** pinion's citation convention is a *provenance* relation; Path
B models an *implementation* relation; the two do not coincide. Reject the
mechanical binding. `severity_binding` stays `info` (no `mnemosyne.toml`
change). Recorded as meta/tooling (mirrors MNEMO-GAP-001, which is infra
commits with no product changelog entry — a fabricated `§impact` here would
itself be a false binding).

**Doors left open (both deferred, evidence-first):**

1. **Option-C, if traceability is ever a stated goal** — per-section
   curation (49 sections), *gated on* first cleaning citation semantics
   (resolve the `§5.16=896` boilerplate-default; see carry below). §2–§4 are
   that workplan.
2. **Mnemosyne-upstream RFC: a "provenance-citation" mode** that distinguishes
   `references §X` (provenance, no bind) from `implements §X` (binds), so the
   binding gate could become useful for pinion without demanding global
   set-equality. Carry as a Mnemosyne-upstream item (parallel to SCE-004);
   not actioned in this repo by R716.

**Carry (citation-hygiene debt, fix-on-touch — NOT part of this decision):**
the `§5.16=896` bucket is a meaningless boilerplate-default co-citation (the
860-count drift MNEMO-GAP-001 §3 flagged). It is a hygiene debt to correct
opportunistically when files are touched (most `§5.16` cites should become
their real section — `§5.38` widgets, `§5.2`/`§5.11` scene, `§5.40` a11y, …
— or be dropped). It is a *prerequisite* for any future Option-C, not a task
mandated by Option A.

## 0. Where this came from

MNEMO-GAP-001 closed the **hallucination-class** citation gate
(`section_missing`=0, `missing`=0, `severity_missing="reject"`, wired into
pre-commit/pre-push). The **binding-class** (Path B) axis was deliberately
left at `severity_binding="info"` — this doc scopes whether/how to adopt it.

## 1. THE DECISION (do this first — do NOT skip to binding)

`validate-code-refs` Path B models a **bidirectional set-equality** between a
spec section `§X` and the code files that implement it:

- `CitationUnbound` — file F cites `§X` but F not in `§X.implementations`
- `ImplementationUnbacked` — F in `§X.implementations` but F has no `§X` cite
- `ImplementationMissing` — `§X` is active but `implementations` is empty

This model assumes **a `§X` citation in code is an implementation claim**
("F implements §X"). pinion's citations are **NOT that** — they are
*provenance* pointers in the `R<round> §<section> — <desc>` header convention
("this code relates to round R<n> / spec-area §<x>"). Evidence (decisive):

- `§5.16` ("GPU renderer architecture") is cited **896 times** — the single
  largest `CitationUnbound` bucket — by `Cargo.toml`, `composite_tag.rs`,
  `reactive/owner.rs` (owner tick), … **none of which implement a GPU
  renderer.** `§5.16` is a meaningless boilerplate-default co-citation (the
  same 860-count drift MNEMO-GAP-001 §3 flagged as fix-on-touch).

**So binding pinion's citations as-is would codify ~896 false "implements
GPU renderer" claims into the spec.** Path B's model and pinion's citation
philosophy are, today, **incompatible**. The next session must choose:

| Option | What it means | Cost | Honest read |
|---|---|---|---|
| **A. Reject Path B for pinion** | Declare §-citations = *provenance, not implementation*; keep `severity_binding="info"` (or work with the Mnemosyne maintainer on a "provenance-citation" mode that doesn't demand set-equality). | ~0 | Likely correct. `[[verify-seed-claims-audit-first]]` applied to the roadmap item: "adopt Path B" may itself be the wrong goal. |
| **B. Adopt with a citation taxonomy** | Split every citation into `implements §X` (binds) vs `references §X` (provenance, no bind, distinct marker). | Very high (per-citation reclassification of ~3.5k) + §5.16-class cleanup first | Only if true spec↔code traceability is a stated goal. |
| **C. Per-section curation** | For each of the 49 cited sections, hand-pick the files that genuinely implement it; bind those; reclassify/remove the rest. | High but bounded (49 sections) | The middle path if traceability is wanted without a global taxonomy. |

**Recommendation: A (or A-leaning).** pinion's provenance-citation convention
is legitimate and useful; do **not** mechanically bind 3 465 citations — it
would encode boilerplate as spec truth. If spec↔code traceability is later
wanted, do it as a deliberate **Option-C** effort, section by section, gated
on first cleaning citation semantics (§5.16 etc.). This is a *framing audit*,
not a mechanical task.

## 2. Scope (measured 2026-05-31, post-MNEMO-GAP-001)

```
violations: citation_unbound=3465  impl_unbacked=81  impl_missing=33
  unique §sections cited (unbound): 49
  unique files (unbound):           359
  top buckets: §5.16=896  §5.49=271  §5.50=199  §2=186  §5.22=185
               §5.38=184  §5.40=141  §5.45=137
  impl_missing (active section, zero implementations): 33 sections —
    §1 §2 §4 §5 §5.1 §5.2 §5.4 §5.5 §5.6 §5.8 §5.9 §5.10 §5.11 §5.13 §5.14
    §5.17 §5.18 §5.19 §5.24 §5.25 §5.26 §5.27 §5.29 §5.30 §5.31 §5.37 §5.37.5
    §5.37.6 §5.37.7 §6 §6.1 §6.2 §6.4
```

The `§5.16=896` bucket is mostly noise (boilerplate) and **must be resolved
before** it could ever be bound — it is the dominant blocker.

## 3. If adopting (Option B/C) — prerequisites + workflow

1. **PREREQ — citation semantic cleanup.** Resolve the `§5.16` boilerplate
   (896) and any other miscited sections so each *remaining* citation truly
   means "implements." Most `§5.16` cites should become their real section
   (`§5.38` widgets, `§5.2`/`§5.11` scene, etc.) or be dropped. This is the
   maintainer's deferred fix-on-touch, at scale. Re-measure after.
2. **Bind.** `mnemosyne-cli add-section-implementation --section §N
   --file <workspace-relative-posix-path> [--symbol Sym]`. **One (section,
   file) per call** — no batch/file-input mode exists, so this needs a
   driver script that parses the (corrected) `CitationUnbound` list and emits
   one call per pair. Derive bindings *only* from genuine-implementation
   citations (post-cleanup), never from the raw scan.
3. **Close the spec side.** `ImplementationMissing` (33) clears as each active
   section gains ≥1 implementation; `ImplementationUnbacked` (81) means a
   registered impl file no longer cites the section — audit those for stale
   bindings.
4. **Promote.** Only once all three Path B counts are 0, set
   `severity_binding="reject"` in `mnemosyne.toml` (the pre-commit/pre-push
   wire already runs `validate-code-refs`, so reject takes effect immediately).

## 4. Commands to re-measure

```
mnemosyne-cli validate-code-refs 2>&1 | grep 'violations: total'
# bucket by section:
mnemosyne-cli validate-code-refs 2>&1 | grep '\[citation_unbound\]' \
  | grep -oE '§[0-9.]+[a-z]?' | sort | uniq -c | sort -rn
```
