//! Rust source emitter. R38 ratify (one file = one struct):
//!
//! ```rust,ignore
//! pub struct <Name> {
//!     pub <signal_field>: ::pinion_core::reactive::Signal<<ty>>,
//!     // ... one field per <signal> child
//! }
//!
//! impl <Name> {
//!     pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {
//!         Self {
//!             <signal_field>: ::pinion_core::reactive::Signal::new(<initial>),
//!             // ...
//!         }
//!     }
//! }
//! ```
//!
//! With zero `<signal>` children the struct collapses to a unit type
//! and the constructor returns `Self`. The owner parameter is always
//! present in the constructor signature (downstream callers always have
//! a real `Owner` in scope) but is bound as `_owner` until R38.2b lands
//! `<computed>` (which needs current-owner registration via the
//! thread-local stack inside `Computed::recompute`).
//!
//! The emitted code is intentionally string-formatted (not via `syn` /
//! `quote`). The template grows linearly in `<signal>` count; the
//! trade-off versus a token-tree builder is re-evaluated when
//! `<computed>` / `<resource>` introduce closure bodies that benefit
//! from real expression-level escaping.

use crate::ast::{PinionChild, PinionDoc, PinionKind, SignalDecl};

const INDENT: &str = "    ";

/// Render `doc` to a self-contained Rust source string. The output is
/// valid input for `rustc` / `cargo check` and is `cargo fmt`-stable
/// (no trailing whitespace, single trailing newline).
#[must_use]
pub fn emit_rust(doc: &PinionDoc) -> String {
    match doc.kind {
        PinionKind::Reactive => emit_reactive(doc),
    }
}

fn emit_reactive(doc: &PinionDoc) -> String {
    let name = &doc.name;
    let signals: Vec<&SignalDecl> = doc
        .children
        .iter()
        .map(|c| match c {
            PinionChild::Signal(s) => s,
        })
        .collect();

    if signals.is_empty() {
        return emit_unit_struct(name);
    }
    emit_struct_with_signals(name, &signals)
}

fn emit_unit_struct(name: &str) -> String {
    format!(
        "pub struct {name};\n\
         \n\
         impl {name} {{\n\
         {INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n\
         {INDENT}{INDENT}Self\n\
         {INDENT}}}\n\
         }}\n"
    )
}

fn emit_struct_with_signals(name: &str, signals: &[&SignalDecl]) -> String {
    let mut fields = String::new();
    let mut inits = String::new();
    for sig in signals {
        fields.push_str(&format!(
            "{INDENT}pub {field}: ::pinion_core::reactive::Signal<{ty}>,\n",
            field = sig.name,
            ty = sig.ty,
        ));
        inits.push_str(&format!(
            "{INDENT}{INDENT}{INDENT}{field}: ::pinion_core::reactive::Signal::new({initial}),\n",
            field = sig.name,
            initial = sig.initial,
        ));
    }
    format!(
        "pub struct {name} {{\n\
         {fields}\
         }}\n\
         \n\
         impl {name} {{\n\
         {INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n\
         {INDENT}{INDENT}Self {{\n\
         {inits}\
         {INDENT}{INDENT}}}\n\
         {INDENT}}}\n\
         }}\n"
    )
}
