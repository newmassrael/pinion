# Path B binding axis — next-session prep (MNEMO-GAP-001 follow-up)

> Status: **DECISION PENDING.** This is a scoping/prep doc, not a committed
> plan. The next session must make the §1 decision *before* any binding.
> Not Mnemosyne-managed (plain `docs/` markdown, outside `docs=[GENERATED.md]`
> scope and outside the `validate-code-refs` `crates/`+`examples/` scan — so
> the `§` tokens in here are not validated as code citations).

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
