# SCE RFC 001 — Maintainer Response

**Status**: Response (SCE-side answers + RFC revision requests)
**RFC**: `sce-rfc-001-downstream-infrastructure.md`
**Response date**: 2026-05-16
**Responding repo**: `scxml-core-engine` @ `main`

---

## 0. TL;DR

Both asks accepted with caveats. The two clarifications you requested have landed as documentation patches on SCE `main`:

- **Ask A answer** → `ARCHITECTURE.md` § "Code Generator: sce-codegen" → new subsection **"Stability and Library Use"**.
- **Ask B answer** → `SCE_FORGE.md` §3.1 → new subsection **"Foreign Namespace Policy (non-`sce:` extensions)"**.

Short version:

- **Ask A**: `sce-build` is published as `rlib`+`cdylib` already. Library use is supported. **But until SCE 1.0 every `pub` item is unstable** — pin a specific commit and treat the parser/IR surface as private-by-policy. A formal stability tier will be declared in a future SCE release alongside the 1.0 cut; this RFC is not the venue for it.
- **Ask B**: XSD **preserves** foreign-NS nodes (`##any`/`##other`, `processContents="lax"`); the IR **drops** them (no model slot). Behavior #1 from your enumeration, not your preferred #2. We are documenting current behavior, not committing to change it.

Sections 1–3 below answer the asks. Section 4 lists six revisions we'd like on the RFC itself before it is treated as a maintainer-signed-off reference. Section 5 covers what we are explicitly **not** committing to.

---

## 1. Ask A — `sce-build` library mode

### 1.1 Sufficient answer is already in the tree

`sce-build/Cargo.toml`:
```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

`sce-build/src/lib.rs` doc string:
> "Drop into build.rs to eliminate the Python codegen dependency."

`forge/mod.rs` exposes `pub mod model | parser | provenance | sourcemap | xsd_validator | diagnostic | target_plugin` — all already `pub`, none `#[doc(hidden)]`. So the visibility question the RFC asks ("are these library-accessible?") has been **yes** since before this RFC was filed.

The question that actually needs a maintainer ruling is the one the RFC asks implicitly: **what is the stability commitment on those public items?**

### 1.2 Answer: unstable until 1.0

Verbatim from the new `ARCHITECTURE.md` subsection:

> Until SCE 1.0, every `pub` item in `sce-build` is unstable and may change between commits without notice or migration path. This includes `forge::model`, `forge::parser`, `forge::provenance`, `forge::sourcemap`, `forge::diagnostic`, `forge::xsd_validator`, and `forge::target_plugin`. This is policy, not oversight: 5-backend codegen parity and the v1 diagnostic wire contract are still consolidating, and freezing the surface before they settle would force a back-compat shim later.

The only two surfaces with their own pre-1.0 governance today are:

1. The `--error-format=json` NDJSON wire contract (`SCE_ERROR_CONTRACT.md` + `schemas/sce-diagnostic.v1.schema.json`).
2. `sce-codegen` CLI flags (touched intentionally rather than incidentally).

Everything else churns.

### 1.3 Practical guidance for pinion

- Pin a specific SCE commit in your `Cargo.toml` or vendor it.
- Treat `forge::*` as **private-by-policy** even though it is `pub`. We will not file a deprecation cycle when we move things.
- Plan to re-vendor on every SCE bump until the SCE 1.0 stability declaration ships.
- If a specific module ends up load-bearing for pinion and we move it, **file a fixture**: a small repro showing pinion's build depends on the moved symbol, ideally as a minimal `cargo test` against pinned SCE. We will review it case-by-case and, if accepted, add a focused regression test under `sce-build/tests/` so the next move surfaces an explicit migration note rather than a silent break. That is not a back-compat promise, an SLA, or a commitment to maintain a dedicated consumer-fixture directory — it is a coordination handle case-by-case.

### 1.4 We are rejecting the §2.4 IR-JSON fallback

The RFC offered an alternative: "if `sce-build` is firmly CLI-only, commit to maintaining an IR JSON export." We are choosing **neither** of those two options:

- `sce-build` is **not** CLI-only — it is a library, see above.
- We are therefore **not** committing to an `--emit ir-json` export. Building one would be a new feature, not a clarification, and the right consumer of the IR is the library, not a serialization round-trip.

If a future need for IR JSON arises (e.g. cross-language tooling that cannot link Rust), it is a separate RFC.

---

## 2. Ask B — Custom namespace tolerance

### 2.1 Answer: stage-split

From the new `SCE_FORGE.md` §3.1 subsection (paraphrased):

| Stage | Behavior on foreign-NS |
|-------|------------------------|
| XSD validation | **Preserve** (`##any` / `##other`, `processContents="lax"`) — no diagnostic raised. |
| SCXML → IR parsing | **Drop** — parser-helper functions filter children by both local element name AND namespace via `is_scxml_ns`. Foreign-NS elements are dropped uniformly, whether their local name is novel or collides with a W3C name. |
| Forge kind parsing | **Drop** — explicit `Some(SCE_NAMESPACE)` filter. |

So the answer to "behavior #1, #2, or #3?" is: **XSD = #2 (preserve), IR = #1 (drop)**, with a consistent uniform drop at the IR boundary regardless of local-name choice.

### 2.2 Local-name collisions: fixed at the root, not documented as a caveat

The original land of this response carried a caveat row noting that a foreign-NS element whose local name collides with a W3C name (e.g. `<framework:onentry>`) would be matched as if it were the W3C element, because the parser dispatched by local name only. Pinion is a real foreign-NS consumer; under CLAUDE.md "Root cause only", deferring a documented footgun for a known consumer was the wrong call.

The fix is a parser-helper hardening: `scxml_child` / `scxml_children` in `sce-build/src/parser.rs` now filter by both local name and namespace via a new `is_scxml_ns(node)` predicate (lenient on missing namespace declarations to preserve legacy fixtures). 32 call sites unchanged — the fix is entirely inside the two helpers. Full `cargo test -p sce-build` suite (1,043 + 863 + 38 integration crates) passes with zero regression.

Practical implication for pinion: **no local-name reservation list applies.** `<framework:state>`, `<framework:onentry>`, `<framework:transition>` — all of these are correctly dropped from the IR. Pinion can use any foreign-NS local name without colliding with W3C SCXML semantics.

### 2.3 Why not behavior #2

The RFC's preferred behavior — "preserve foreign nodes in a foreign-branch of the AST so downstream tooling can read them" — would require:

1. New variant(s) on `SCXMLModel` IR nodes for unrecognized children.
2. Roundtripping foreign-NS attribute carriers through `forge::model` field-by-field.
3. A stability commitment on the resulting IR shape *before* we know which fields downstream consumers will reach for, which is the exact mistake `feedback_planned_not_yagni` warns against.

We are not doing any of this on speculation. If pinion's framework needs foreign-NS annotations on SCXML nodes, the textbook path is the one your RFC §3.3 already lists as a fallback: **separate files, or different file extensions for framework DSL.** That keeps SCE's IR honestly about SCXML and pushes framework concerns into pinion's own parser pass.

### 2.4 Diagnostic interaction (§3.4): rejected

You floated a `validation/unknown-namespace` diagnostic code "for clarification." Adding a new `DiagnosticCode` variant has an 11-place sync edit checklist in SCE; new variants are introduced when SCE itself needs to fire them, not pre-emptively for downstream cases. Since current behavior **does not reject** foreign-NS nodes at any stage, there is no diagnostic to name. If/when SCE adds rejection behavior, that's the moment a code would be coined.

---

## 3. What this response does not commit to

To match your RFC §4 explicitly:

- No new `sce:kind`.
- No framework template in `tools/codegen/templates/`.
- No framework API in `sce-rust-runtime`.
- No new `DiagnosticCode` in the closed enum.
- No transport binding beyond what `SCE_MESH.md` already specifies.
- **No back-compat commitment on `sce-build` pub items pre-1.0.**
- **No commitment to add a foreign-nodes branch to `SCXMLModel`.**
- **No `--emit ir-json` mode.**

---

## 4. Revision requests on the RFC document

We are happy to have this RFC referenced as the canonical statement of how pinion integrates with SCE, but the current text would mislead a future reader on three points. Please apply the following before treating it as maintainer-signed-off:

### 4.1 TL;DR is inconsistent with §2.4, §3.3, §3.4

The TL;DR claims "API exposure / documentation request — no behavior change requested." Three sub-asks in the body actually request behavior changes:

- §2.4 — "commit to maintaining an IR JSON export" = new emit mode.
- §3.3 — preferred behavior #2 = IR shape change.
- §3.4 — new `validation/unknown-namespace` `DiagnosticCode` = closed-enum edit.

**Fix**: split the TL;DR into two buckets — *clarifications* (two), and *scoped follow-ups that would be separate RFCs* (the three above). This response only addresses the clarifications.

### 4.2 §2 should drop the "is `sce-build` a library?" framing

The Cargo manifest already answers this — `crate-type = ["cdylib", "rlib"]` shipped before the RFC was filed. Asking the question makes the RFC read as if pinion hasn't checked the source.

**Fix**: rewrite §2.1 / §2.2 to ask only the question that actually needs a ruling — **stability tier** — and cite the Cargo manifest line.

### 4.3 §2.4 should be deleted or moved to a separate RFC

It proposes a new SCE feature (`--emit ir-json`). Keeping it in an RFC labeled "no behavior change requested" is misleading.

**Fix**: delete §2.4. If pinion later needs IR JSON, file a dedicated RFC.

### 4.4 §3.3 should drop the "non-blocking opinion" framing

The opinion preferring behavior #2 *would block* if acted on (IR shape change). The actual non-blocking ask in §3.3 is the final clause: "we just need to know which game we're playing."

**Fix**: cut the "we'd prefer #2" paragraph. Replace with: "Document current behavior. We will live with whichever stage-split SCE has."

### 4.5 §3.4 should be deleted

New `DiagnosticCode` variant requests are not clarifications under SCE's error-contract governance.

**Fix**: delete §3.4.

### 4.6 §6 "Suggested resolution" should accept a deferred-stability outcome

Right now §6 implies SCE can answer with a single doc patch *or* a public API audit. The honest third option — "answer the *current behavior* via doc patches and defer the *stability commitment* to a future SCE release" — is the one we are taking for the Ask A portion. The doc patches in this response cover the current state of both surfaces; the formal stability tier will land alongside SCE 1.0, not in response to a single downstream RFC.

**Fix**: add a third bullet under §6: "OR defer the stability tier portion to a future SCE release and accept doc patches as the interim answer."

---

## 5. Open questions in the RFC §5 — closed

| # | RFC question | Answer |
|---|--------------|--------|
| 1 | Is `sce-build` intended as a library, CLI, or both? | **Both.** `rlib` + `cdylib` + `sce-codegen` binary all consume the same crate. |
| 2 | Which modules in `forge/` are public-stable vs internal? | **None public-stable pre-1.0.** All are `pub` for workspace reasons; treat as private-by-policy. |
| 3 | Current behavior on custom-namespace elements/attributes? | XSD preserves, IR drops. See §2.1 above and `SCE_FORGE.md` §3.1. |
| 4 | Is there a convention for downstream `DiagnosticCode` prefixes? | **No, and not planned.** The enum is closed-by-design per `SCE_ERROR_CONTRACT.md`. Downstream tooling that needs its own diagnostic vocabulary must define its own type, not extend SCE's. |

---

## 6. What pinion can rely on

After this response and the two doc patches:

- `sce-build` is yours to depend on as a Rust library. Pin a commit.
- Foreign-NS annotations on SCXML nodes will not raise diagnostics, but will not appear in SCE's IR — read them from the source XML yourself.
- The v1 diagnostic NDJSON wire contract is the one SCE-side guarantee you can plan around long-term.
- Anything else in `sce-build/src/` may move without notice until SCE 1.0.

If the framework's evolution surfaces a specific symbol whose churn would cost you a lot, file a fixture and we'll add it to the cross-impact surface as described in §1.3. That is the maximum coordination handle we are extending pre-1.0.

---

## 7. Acknowledgement

The RFC's framing — "SCE remains universal infrastructure, pinion brings its own DSL" — is exactly the right shape and we appreciate it being filed as a question rather than an upstream-the-framework patch. The revisions in §4 are about precision, not direction.
