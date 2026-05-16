//! Rust source emitter. R38 ratify (one file = one struct).
//!
//! ## R38.2b emission shape
//!
//! ```rust,ignore
//! pub struct <Name> {
//!     pub <signal>: ::pinion_core::reactive::Signal<<ty>>,
//!     pub <computed>: ::pinion_core::reactive::Computed<<ty>>,
//!     // ... in declaration order
//! }
//!
//! impl <Name> {
//!     pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {
//!         let <signal> = ::pinion_core::reactive::Signal::new(<initial>);
//!         let <computed> = {
//!             #[allow(unused_variables, clippy::redundant_clone)]
//!             let (<prior>,) = (<prior>.clone(),);
//!             ::pinion_core::reactive::Computed::new(move || { <body> })
//!         };
//!         Self { <signal>, <computed> }
//!     }
//! }
//! ```
//!
//! ## Capture policy
//!
//! `<computed>` closure capture is **over-capture by clone**: every
//! prior child identifier in the same `<pinion>` document is shadowed
//! by a `.clone()` binding right before the `move` closure, regardless
//! of whether the body actually references it. The unused-variable
//! warning is suppressed at the inner block.
//!
//! Rationale: the runtime reactive graph (R26 push-pull) discovers the
//! *real* dependency set when the closure first calls `Signal::get()` /
//! `Computed::get()`, so a static dependency list at codegen would be
//! either redundant or a footgun. The over-capture is just what Rust's
//! borrow checker needs — the closure must own its captures because
//! `Self { ... }` afterwards moves the originals.
//!
//! Precise `syn::Expr` analysis to capture only the referenced
//! identifiers is a [carry-forward R38.2x] decision; cost-benefit
//! re-evaluated when `<resource>` lands and the closure shape grows.
//!
//! ## String formatting choice
//!
//! Source is string-formatted (not via `syn` / `quote`). Body
//! expressions are *not* parsed — they pass through verbatim. Syntax
//! validation is delegated to `rustc` at the consumer's `cargo build`.

use crate::ast::{ComputedDecl, PinionChild, PinionDoc, PinionKind, SignalDecl};

const INDENT: &str = "    ";

/// Render `doc` to a self-contained Rust source string. Output is
/// valid `rustc` input and `cargo fmt`-stable (single trailing newline).
#[must_use]
pub fn emit_rust(doc: &PinionDoc) -> String {
    match doc.kind {
        PinionKind::Reactive => emit_reactive(doc),
    }
}

fn emit_reactive(doc: &PinionDoc) -> String {
    let name = &doc.name;
    if doc.children.is_empty() {
        return emit_unit_struct(name);
    }
    emit_struct_with_children(name, &doc.children)
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

fn emit_struct_with_children(name: &str, children: &[PinionChild]) -> String {
    let mut fields = String::new();
    let mut bindings = String::new();
    let mut self_inits = String::new();

    // Names introduced by prior children — used as the over-capture set
    // for each subsequent <computed> closure. Order matters: the user
    // sees declarations evaluated top-to-bottom, so dependencies in a
    // <computed> body must reference earlier children only.
    let mut prior_names: Vec<String> = Vec::new();

    for child in children {
        match child {
            PinionChild::Signal(s) => emit_signal_into(s, &mut fields, &mut bindings, &mut self_inits),
            PinionChild::Computed(c) => {
                emit_computed_into(c, &prior_names, &mut fields, &mut bindings, &mut self_inits);
            }
        }
        prior_names.push(child_name(child).to_owned());
    }

    format!(
        "pub struct {name} {{\n\
         {fields}\
         }}\n\
         \n\
         impl {name} {{\n\
         {INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n\
         {bindings}\
         {INDENT}{INDENT}Self {{\n\
         {self_inits}\
         {INDENT}{INDENT}}}\n\
         {INDENT}}}\n\
         }}\n"
    )
}

fn emit_signal_into(s: &SignalDecl, fields: &mut String, bindings: &mut String, inits: &mut String) {
    fields.push_str(&format!(
        "{INDENT}pub {field}: ::pinion_core::reactive::Signal<{ty}>,\n",
        field = s.name,
        ty = s.ty,
    ));
    bindings.push_str(&format!(
        "{INDENT}{INDENT}let {field} = ::pinion_core::reactive::Signal::new({initial});\n",
        field = s.name,
        initial = s.initial,
    ));
    inits.push_str(&format!("{INDENT}{INDENT}{INDENT}{field},\n", field = s.name));
}

fn emit_computed_into(
    c: &ComputedDecl,
    prior_names: &[String],
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    fields.push_str(&format!(
        "{INDENT}pub {field}: ::pinion_core::reactive::Computed<{ty}>,\n",
        field = c.name,
        ty = c.ty,
    ));

    if prior_names.is_empty() {
        // No prior children to capture — emit the plain closure form.
        // `move ||` still allowed (and idiomatic) because the closure
        // may later be passed to a `'static`-bounded scheduler.
        bindings.push_str(&format!(
            "{INDENT}{INDENT}let {field} = \
             ::pinion_core::reactive::Computed::new(move || {{ {body} }});\n",
            field = c.name,
            body = c.body,
        ));
    } else {
        // Tuple-shadow over-capture. Single-name tuples use the
        // trailing-comma form to stay grammatically uniform with the
        // multi-name case. See module docs for the capture-policy
        // rationale.
        let lhs = format_tuple(prior_names.iter().map(String::as_str));
        let rhs =
            format_tuple(prior_names.iter().map(|n| format!("{n}.clone()")).collect::<Vec<_>>().iter().map(String::as_str));
        bindings.push_str(&format!(
            "{INDENT}{INDENT}let {field} = {{\n\
             {INDENT}{INDENT}{INDENT}#[allow(unused_variables, clippy::redundant_clone)]\n\
             {INDENT}{INDENT}{INDENT}let {lhs} = {rhs};\n\
             {INDENT}{INDENT}{INDENT}::pinion_core::reactive::Computed::new(move || {{ {body} }})\n\
             {INDENT}{INDENT}}};\n",
            field = c.name,
            body = c.body,
        ));
    }

    inits.push_str(&format!("{INDENT}{INDENT}{INDENT}{field},\n", field = c.name));
}

/// Build a parenthesized tuple literal. Single-element tuples use
/// `(x,)` form per Rust grammar; zero-element should never reach here
/// (the caller guards with `is_empty`).
fn format_tuple<'a, I>(items: I) -> String
where
    I: IntoIterator<Item = &'a str>,
{
    let collected: Vec<&str> = items.into_iter().collect();
    debug_assert!(!collected.is_empty(), "format_tuple called with empty iter");
    if collected.len() == 1 {
        format!("({},)", collected[0])
    } else {
        format!("({})", collected.join(", "))
    }
}

fn child_name(child: &PinionChild) -> &str {
    match child {
        PinionChild::Signal(s) => &s.name,
        PinionChild::Computed(c) => &c.name,
    }
}
