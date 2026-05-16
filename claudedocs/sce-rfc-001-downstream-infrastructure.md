# SCE RFC 001 — Downstream Framework Integration Surface

**Status**: Revised 2026-05-16 per maintainer response (`sce-rfc-001-response.md` §4)
**Original draft**: pinion-side authored, filed against `scxml-core-engine` @ `main`
**Audience**: SCE maintainers
**Type**: Documentation request — current-state clarification on two surfaces
**Author**: Downstream framework consumer (pinion GUI framework)

---

## 0. TL;DR

We are building a downstream GUI framework that embeds `sce-rust-runtime`
and wants to use SCE as the **universal codegen infrastructure** layer
underneath a framework-specific DSL. `SCE_FORGE.md` already positions
SCE that way ("universal intermediate representation for multi-language
code generation") — this RFC asks for two current-state clarifications
so downstream frameworks can plug in cleanly without forking SCE or
upstreaming framework-specific code into the SCE core.

### Clarifications requested by this RFC (in scope)

1. **`sce-build` stability tier** — the Cargo manifest already publishes
   the crate as `rlib`+`cdylib`, so the library-vs-CLI question is
   settled; what needs a maintainer ruling is the *stability commitment*
   attached to the public items in `forge/`.
2. **Custom-namespace tolerance documentation** — what is `sce-build`'s
   current behavior when an SCXML document contains elements/attributes
   in a non-`sce:` namespace (e.g. `xmlns:framework="…"`)? Silently
   skip, preserve, or reject? Pinion needs the answer documented, not
   changed.

### Scoped follow-ups that would be separate RFCs (out of scope here)

The first draft of this RFC included three items that — on maintainer
review — turned out to request *behavior changes* rather than
clarifications. They have been removed from the current document. If
pinion later needs any of them, each is its own RFC:

- An `sce-codegen --emit ir-json` export mode (was §2.4).
- IR shape change to preserve foreign-namespace nodes in `SCXMLModel`
  (was §3.3 "preferred behavior #2").
- A new `validation/unknown-namespace` `DiagnosticCode` variant
  (was §3.4).

We are **not** asking SCE to take on any framework-specific kind,
codegen template, or runtime API. Our framework will own its DSL,
schema, and Rust emitter in a separate crate; SCE remains the universal
infrastructure.

---

## 1. Context

`SCE_FORGE.md` §1 states:

> One SCXML, all languages. Extended SCXML is the single source of truth
> for any codegen-able pattern — not just state machines. The kind system
> makes SCXML a universal intermediate representation for multi-language
> code generation.

The current kind catalog covers cross-domain universal patterns:
`statechart`, `transform`, `lookup`, `condition`, `procedure`, `codec`,
`validator`, `filter`, `interpolation`, `timer`, `observer`, plus
mesh/build-time kinds (`link`, `worker`, `buffer-pool`). These are all
patterns multiple downstream consumers would need.

Downstream frameworks (UI frameworks, robotics middleware, etc.) tend to
have authoring surfaces that are **not** cross-domain — e.g. a UI
framework's fine-grained reactivity DSL is specific to that framework's
runtime API. Putting framework-specific kinds into SCE Forge would dilute
SCE's universal positioning. Conversely, having every framework fork SCE
to add its own kinds would fragment the ecosystem.

The textbook resolution: SCE remains universal infrastructure, framework
brings its own DSL crate, and the two compose at the parser/IR boundary.
This RFC asks how that boundary is shaped today.

---

## 2. Ask A — Stability tier of the `sce-build` library surface

### 2.1 The library/CLI question is already answered

`sce-build/Cargo.toml`:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

`sce-build/src/lib.rs` docstring documents it as a `build.rs` drop-in.
`forge/mod.rs` exposes `pub mod model | parser | provenance | sourcemap |
xsd_validator | diagnostic | target_plugin` — all `pub`, none
`#[doc(hidden)]`. So library use is already on the table; pinion does
not need a ruling on whether it is *allowed*, only on what it *means*.

### 2.2 The ask

For each of the following modules in `sce-build/src/forge/`, what is
the maintainer's intended stability commitment pre-1.0?

| Module | Question |
|---|---|
| `model.rs` (`ForgeDocument` enum, kind variants) | Stable, unstable, internal-by-policy? |
| `parser.rs` (Extended SCXML → ForgeDocument) | Same |
| `provenance.rs` (source location tracking) | Same |
| `sourcemap.rs` (sourcemap emission) | Same |
| `xsd_validator.rs` (schema validation) | Same |
| `diagnostic.rs` (v1 wire format emission) | Same |
| `target_plugin.rs` (codegen target registration) | Same — and: can downstream crates register new emit targets? |

### 2.3 What we are *not* asking for

- A new API surface — current surface is fine; we just need the
  stability framing.
- A pre-1.0 stability guarantee in absolute terms — pre-1.0 evolution
  is expected, we just need to know the policy.
- Pinion-specific helpers added to `sce-build`.

---

## 3. Ask B — Documenting custom-namespace behavior

### 3.1 Problem statement

W3C SCXML §3.14 allows custom namespaces in SCXML documents. Downstream
frameworks may want to embed framework-specific elements/attributes in
SCXML files (e.g. a UI framework annotating a `<state>` with framework
metadata, or co-locating framework DSL alongside an SCXML statechart in
one file).

`sce-build/src/forge/`'s current behavior on encountering an element or
attribute outside the `sce:` namespace is not currently documented in
`SCE_FORGE.md` or `ARCHITECTURE.md`. Three possible behaviors:

1. **Silently skip** — unknown-namespace nodes dropped from the AST.
2. **Preserve in AST** — unknown-namespace nodes parked under a
   "foreign nodes" branch that downstream tools can read.
3. **Reject** — unknown-namespace nodes raise a validation error.

The behavior may also be **stage-split** — different at XSD validation
vs. SCXML → IR parsing vs. Forge kind parsing.

### 3.2 The ask

Document the current behavior — including any stage-split — in
`SCE_FORGE.md` or `ARCHITECTURE.md`, ideally with a stability statement
("this behavior is intentional / load-bearing / may change pre-1.0").

We will live with whichever behavior SCE has; pinion just needs the
documentation so we can plan our DSL parser accordingly.

---

## 4. What we are explicitly NOT asking

To keep the boundary clean and SCE's universal positioning intact:

- **No new `sce:kind`** for framework-specific patterns (e.g. fine-grained
  reactivity, view functions, UI semantic trees, etc.) — those live in
  the downstream framework's own DSL crate.
- **No framework-specific Rust template** in `tools/codegen/templates/`.
- **No framework-specific API** in `sce-rust-runtime`.
- **No framework-specific `DiagnosticCode`** in the closed enum.
- **No transport bindings** beyond what `SCE_MESH.md` already specifies.
- **No IR JSON export mode** — see §0 scoped follow-ups; if pinion
  later needs this it is a separate RFC.
- **No IR shape change** to carry foreign-namespace nodes — same.

If a future need emerges for cross-framework reactive-style patterns
that multiple downstream consumers would benefit from (analogous to how
`transform` and `lookup` are cross-domain), that is a separate RFC and
a separate discussion. This RFC is scoped purely to "how does a
downstream framework cleanly consume SCE today."

---

## 5. Open questions for SCE maintainers

| # | RFC question | Status |
|---|---|---|
| 1 | What is the stability commitment on `sce-build` `pub` items in `forge/` pre-1.0? | Answered (see §7). |
| 2 | Which modules, if any, have stronger stability commitments than the default? | Answered (see §7). |
| 3 | Current behavior on custom-namespace elements/attributes, including any stage-split? | Answered (see §7). |
| 4 | Is there a convention for downstream tooling that needs its own diagnostic vocabulary? | Answered (see §7). |

---

## 6. Suggested resolution

Either:

- **A single short documentation patch** to `SCE_FORGE.md` and/or
  `ARCHITECTURE.md` answering the open questions above. No code change.
- **OR** a public API audit pass — annotate `pub use` re-exports in
  `sce-build/src/lib.rs` with `#[doc(hidden)]` for internal-only,
  document the rest. Still small.
- **OR** defer the stability-tier portion of Ask A to a future SCE
  release (e.g. alongside the 1.0 cut) and accept doc patches as the
  interim answer.

Either path makes the boundary explicit and unblocks downstream framework
adoption without committing SCE to support specific frameworks.

---

## 7. Maintainer response — received 2026-05-16

Full text archived alongside this RFC at
`sce-rfc-001-response.md`. Short summary:

| Ask | Resolution |
|---|---|
| Ask A (stability tier) | **Unstable until 1.0**. Every `pub` item in `sce-build` may change between commits without migration. Pin a specific commit, treat `forge::*` as private-by-policy, re-vendor on each SCE bump. The only pre-1.0 stable surfaces are the `--error-format=json` NDJSON wire contract and `sce-codegen` CLI flags. Per-symbol coordination handle: file a fixture when a specific module becomes load-bearing for pinion and we're hit by churn. |
| Ask B (namespace behavior) | **Stage-split**. XSD preserves foreign-NS (`##any` / `processContents="lax"`, no diagnostic). SCXML → IR parsing drops foreign-NS (no model slot). Forge kind parsing drops via explicit `Some(SCE_NAMESPACE)` filter. **Local-name collision footgun**: dispatch is by local element name only, so foreign-NS prefixes must avoid the W3C SCXML local-name set (`state`, `transition`, `data`, `onentry`, `onexit`, `parallel`, `final`, `history`, `invoke`, `send`, `cancel`, `raise`, `if`, `elseif`, `else`, `foreach`, `log`, `assign`, `script`, `param`, `content`, `datamodel`, `donedata`, `initial`). |

Doc patches landed on SCE `main`:

- `ARCHITECTURE.md` § "Code Generator: sce-codegen" → new subsection
  **"Stability and Library Use"**.
- `SCE_FORGE.md` §3.1 → new subsection
  **"Foreign Namespace Policy (non-`sce:` extensions)"**.

Items explicitly *not* committed to (from response §3):

- No new `sce:kind`.
- No framework template in `tools/codegen/templates/`.
- No framework API in `sce-rust-runtime`.
- No new `DiagnosticCode` in the closed enum.
- No transport binding beyond `SCE_MESH.md`.
- No back-compat commitment on `sce-build` `pub` items pre-1.0.
- No commitment to add a foreign-nodes branch to `SCXMLModel`.
- No `--emit ir-json` mode.

---

## 8. Pinion-side consumption policy (post-response)

For pinion-forge crate construction:

1. **Dep model**: `sce-build` as `rlib` dependency, **commit-pinned** in
   `Cargo.toml`; not crate-version range. Re-vendor on every SCE bump.
2. **Foreign-NS strategy**: pinion DSL lives in **separate files with a
   distinct extension** (e.g. `.pinion.xml` or `.pscxml`). Not embedded
   inside W3C SCXML documents. This sidesteps the IR-drop behavior and
   the local-name-collision footgun.
3. **Diagnostic system**: pinion-forge defines its **own** diagnostic
   type, not an extension of SCE's `DiagnosticCode` enum. We may model
   our wire format after the SCE v1 NDJSON contract, but the namespace
   is pinion's, not SCE's.
4. **Per-symbol coordination**: if a specific symbol in `forge::*`
   becomes load-bearing for pinion-forge and SCE churn breaks us, file
   a focused fixture per response §1.3.

---

## 9. Acknowledgements

This RFC was filed in good faith from a downstream consumer that has
benefited significantly from SCE's existing infrastructure (statechart
codegen, sourcemap, v1 diagnostic wire contract). The "universal codegen
infrastructure" framing in `SCE_FORGE.md` §1 is exactly the right
positioning — this RFC just asks for the integration surface to be
documented so downstream consumers can rely on it long-term.

Thanks to SCE maintainers for the prompt response and the careful
revision review. The original draft slipped behavior-change requests
into a "clarification" framing; the precision feedback in response §4
has been applied to this revision.
