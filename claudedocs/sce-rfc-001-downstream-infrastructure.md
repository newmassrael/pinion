# SCE RFC 001 — Downstream Framework Integration Surface

**Status**: Draft (pinion-side authored, pending upstream submission)
**Audience**: SCE maintainers
**Type**: API exposure / documentation request — no behavior change requested
**Author**: Downstream framework consumer (pinion GUI framework)

---

## 0. TL;DR

We are building a downstream GUI framework that embeds `sce-rust-runtime`
and wants to use SCE as the **universal codegen infrastructure** layer
underneath a framework-specific DSL. SCE's `SCE_FORGE.md` already positions
itself this way ("universal intermediate representation for multi-language
code generation") — this RFC asks for two small clarifications/exposures
so downstream frameworks can plug in cleanly without forking SCE or
upstreaming framework-specific code into the SCE core.

**Asks** (both are clarification/exposure, not new features):

1. **`sce-build` library-mode surface** — can downstream Rust crates depend
   on `sce-build` as a library and access the parsed `ForgeDocument` model
   directly, or is `sce-codegen` the only intended consumer entry point?
2. **Custom namespace tolerance** — what is `sce-build`'s documented
   behavior when an SCXML document contains elements/attributes in a
   non-`sce:` namespace (e.g. `xmlns:framework="…"`)? Silently skip,
   preserve in AST, or reject?

We are **not** asking SCE to take on any framework-specific kind, codegen
template, or runtime API. Our framework will own its DSL, schema, and Rust
emitter in a separate crate; SCE remains the universal infrastructure.

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

## 2. Ask A — `sce-build` library-mode surface

### 2.1 Problem statement

We want our framework's `build.rs` to:

1. Discover framework-specific schema files in the user's project tree
2. Parse them (potentially as Extended SCXML with framework namespace,
   or potentially as a separate file format)
3. Emit framework-specific Rust runtime code

If we parse as Extended SCXML, we would benefit greatly from reusing
SCE's existing parser, validator, source-map emitter, and provenance
infrastructure rather than re-implementing them. Today, `sce-codegen` is
exposed as a CLI binary (`sce-build/src/bin/`), and we cannot tell from
the public API surface whether `ForgeDocument`, `model::*`, `parser`,
`provenance`, `sourcemap` are intended to be library-stable.

### 2.2 What we'd like documented

For each of the following modules in `sce-build/src/forge/`:

| Module | Question |
|---|---|
| `model.rs` (`ForgeDocument` enum, kind variants) | Is this a library-stable public API, or internal-to-`sce-codegen`? |
| `parser.rs` (Extended SCXML → ForgeDocument) | Same |
| `provenance.rs` (source location tracking) | Same |
| `sourcemap.rs` (sourcemap emission) | Same |
| `xsd_validator.rs` (schema validation) | Same |
| `diagnostic.rs` (v1 wire format emission) | Same |
| `target_plugin.rs` (codegen target registration) | Can downstream crates register new emit targets? |

### 2.3 What we are *not* asking for

- New API surface design — current surface is fine, we just need to know
  what is and isn't stable
- Stable API guarantee in absolute terms — pre-1.0 evolution is expected,
  we just need to know the intent
- Pinion-specific helpers added to `sce-build`

### 2.4 Fallback if library-mode is not intended

If `sce-build` is firmly CLI-only, downstream frameworks would consume
SCE's output via `sce-codegen --emit ir-json` (or similar IR export
mode) and parse the IR JSON from their own build script. We don't think
this is the textbook path — it adds a JSON serialization round-trip
where a library dependency would be cleaner — but it's tolerable. The
RFC asks SCE to choose: (a) document `sce-build` as library-stable, or
(b) commit to maintaining an IR JSON export for downstream consumers.

---

## 3. Ask B — Custom namespace tolerance

### 3.1 Problem statement

W3C SCXML §3.14 allows custom namespaces in SCXML documents. Downstream
frameworks may want to embed framework-specific elements/attributes in
SCXML files (e.g. a UI framework annotating a `<state>` with framework
metadata, or co-locating framework DSL alongside an SCXML statechart in
one file).

`sce-build/src/forge/`'s current behavior on encountering an element or
attribute outside the `sce:` namespace is not documented in
`SCE_FORGE.md` or `ARCHITECTURE.md` (we checked). Three possible
behaviors:

1. **Silently skip** — unknown-namespace nodes dropped from the AST
2. **Preserve in AST** — unknown-namespace nodes parked under a
   "foreign nodes" branch that downstream tools can read
3. **Reject** — unknown-namespace nodes raise a validation error

### 3.2 What we'd like documented

The current behavior, in `SCE_FORGE.md` or `ARCHITECTURE.md`, ideally
with a stability statement ("this behavior is intentional / load-bearing
/ may change pre-1.0").

### 3.3 What we'd prefer (non-blocking opinion)

**Behavior #2 (preserve in AST)** is the textbook composition story:
downstream framework reads its annotations after SCE parses the document.
If today's behavior is #1 or #3 and changing it is non-trivial, we can
work around it (separate files per framework, or different file
extensions) — we just need to know which game we're playing.

### 3.4 Diagnostic interaction (clarification, not ask)

If behavior is #3 (reject), would the rejection raise a
`DiagnosticCode` from the closed `validation/*` enum per
`SCE_ERROR_CONTRACT.md`? If so, having a documented code like
`validation/unknown-namespace` (vs. a generic
`validation/invalid-attribute`) would help downstream tooling
distinguish "spec violation" from "framework annotation SCE doesn't
care about."

---

## 4. What we are explicitly NOT asking

To keep the boundary clean and SCE's universal positioning intact:

- **No new `sce:kind`** for framework-specific patterns (e.g. fine-grained
  reactivity, view functions, UI semantic trees, etc.) — those live in
  the downstream framework's own DSL crate
- **No framework-specific Rust template** in `tools/codegen/templates/`
- **No framework-specific API** in `sce-rust-runtime`
- **No framework-specific `DiagnosticCode`** in the closed enum
- **No transport bindings** beyond what `SCE_MESH.md` already specifies

If a future need emerges for cross-framework reactive-style patterns that
multiple downstream consumers would benefit from (analogous to how
`transform` and `lookup` are cross-domain), that is a separate RFC and a
separate discussion. This RFC is scoped purely to "how does a downstream
framework cleanly consume SCE today."

---

## 5. Open questions for SCE maintainers

1. Is `sce-build` intended as a library, CLI, or both?
2. If both, which modules in `forge/` are public-stable vs. internal?
3. What is the current behavior on custom-namespace elements/attributes?
4. Is there an existing convention for downstream `DiagnosticCode`
   prefixes, or is the closed enum truly SCE-only by design?

---

## 6. Suggested resolution

Either:

- **A single short documentation patch** to `SCE_FORGE.md` and/or
  `ARCHITECTURE.md` answering the four questions above. No code change.
- **OR** a public API audit pass — annotate `pub use` re-exports in
  `sce-build/src/lib.rs` with `#[doc(hidden)]` for internal-only,
  document the rest. Still small.

Either path makes the boundary explicit and unblocks downstream framework
adoption without committing SCE to support specific frameworks.

---

## 7. Acknowledgements

This RFC is filed in good faith from a downstream consumer that has
benefited significantly from SCE's existing infrastructure (statechart
codegen, sourcemap, v1 diagnostic wire contract). The "universal codegen
infrastructure" framing in `SCE_FORGE.md` §1 is exactly the right
positioning — this RFC just asks for the integration surface to be
documented so downstream consumers can rely on it long-term.
