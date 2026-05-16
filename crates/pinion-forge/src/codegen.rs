//! Rust source emitter. R38 ratify (one file = one struct):
//!
//! ```rust,ignore
//! pub struct <name>;
//!
//! impl <name> {
//!     pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {
//!         Self
//!     }
//! }
//! ```
//!
//! The emitted code is intentionally string-formatted (not via `syn` /
//! `quote`). At R38.1 the template is one struct + one impl with zero
//! variation, so introducing a token tree builder adds dependency surface
//! without payoff. When R38.2 introduces `<signal>` / `<computed>` /
//! `<resource>` bodies the trade-off is revisited (the emit template
//! gains real branching at that point).

use crate::ast::{PinionDoc, PinionKind};

/// Render `doc` to a self-contained Rust source string. The output is
/// valid input for `rustc` / `cargo check` and is `cargo fmt`-stable
/// (no trailing whitespace, single trailing newline).
///
/// At R38.1 [`PinionDoc::children`] is always empty by parser construction;
/// this function intentionally does not attempt partial emission for a
/// document with children (impossible at the type level today, and
/// would silently swallow the R38.2 variants when they land).
#[must_use]
pub fn emit_rust(doc: &PinionDoc) -> String {
    match doc.kind {
        PinionKind::Reactive => emit_reactive(doc),
    }
}

fn emit_reactive(doc: &PinionDoc) -> String {
    // PinionChild is uninhabited at R38.1 — the parser cannot construct
    // any variant — so `doc.children` is always empty. Asserting it
    // documents the invariant and turns a future R38.2 variant added
    // without an emit update into a loud debug-build failure.
    debug_assert!(
        doc.children.is_empty(),
        "R38.1 codegen does not yet handle <pinion> children"
    );

    let name = &doc.name;
    format!(
        "pub struct {name};\n\
         \n\
         impl {name} {{\n\
         {INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n\
         {INDENT}{INDENT}Self\n\
         {INDENT}}}\n\
         }}\n",
        INDENT = "    ",
    )
}
