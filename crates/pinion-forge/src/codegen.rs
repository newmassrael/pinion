//! Rust source emitter. R38 ratify (one file = one struct).
//!
//! ## R38.2c emission shape
//!
//! ```rust,ignore
//! pub struct <Name> {
//!     pub <signal>: ::pinion_core::reactive::Signal<<ty>>,
//!     pub <computed>: ::pinion_core::reactive::Computed<<ty>>,
//!     pub <resource>: ::pinion_core::reactive::Resource<<ty>, <err>>,
//!     // ... in declaration order
//! }
//!
//! impl <Name> {
//!     // signature variant A — no <resource> children:
//!     pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self { ... }
//!
//!     // signature variant B — at least one <resource> child:
//!     pub fn new<S>(_owner: &::pinion_core::reactive::Owner, spawner: &S) -> Self
//!     where S: ::pinion_core::reactive::LocalSpawner
//!     { ... }
//! }
//! ```
//!
//! ## Signature policy
//!
//! `<resource>` requires a [`LocalSpawner`] handle at construction
//! time to drive the initial fetch future. Documents with no
//! `<resource>` keep the simpler one-argument `new` so a downstream
//! that only uses signals/computeds is not forced to provide a
//! dummy spawner. The presence of any `<resource>` element widens
//! the signature; the choice is data-driven (children shape) rather
//! than user-toggled.
//!
//! Trade-off vs. always-spawner signature: long-term consistency at
//! the cost of a no-op argument. R38.2c keeps minimum surface; the
//! consistency-first variant is a carry-forward decision to revisit
//! once the dogfood corpus gives empirical signal.
//!
//! ## Capture policy
//!
//! `<computed>` and `<resource>` bodies may reference prior child
//! identifiers. The codegen emits an over-capture shadow block right
//! before the constructor call so the Rust borrow checker accepts the
//! body — runtime tracking (R26 push-pull) discovers the *actual*
//! dependency set at first use.
//!
//! `<computed>` uses `move ||` closure capture; `<resource>` uses
//! `async move { ... }` block capture — both rely on the caller body
//! using `move` semantics. pinion-forge does not wrap the body, so
//! authors must write `async move { ... }` explicitly inside `<resource>`
//! when prior captures are referenced.

use crate::ast::{ComputedDecl, PinionChild, PinionDoc, PinionKind, ResourceDecl, SignalDecl};

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
    // for each subsequent <computed>/<resource> body. Order matters: the
    // user sees declarations evaluated top-to-bottom, so dependencies
    // must reference earlier children only.
    let mut prior_names: Vec<String> = Vec::new();

    for child in children {
        match child {
            PinionChild::Signal(s) => emit_signal_into(s, &mut fields, &mut bindings, &mut self_inits),
            PinionChild::Computed(c) => {
                emit_computed_into(c, &prior_names, &mut fields, &mut bindings, &mut self_inits);
            }
            PinionChild::Resource(r) => {
                emit_resource_into(r, &prior_names, &mut fields, &mut bindings, &mut self_inits);
            }
        }
        prior_names.push(child_name(child).to_owned());
    }

    let signature = if needs_spawner(children) {
        format!(
            "{INDENT}pub fn new<S>(_owner: &::pinion_core::reactive::Owner, spawner: &S) -> Self\n\
             {INDENT}where\n\
             {INDENT}{INDENT}S: ::pinion_core::reactive::LocalSpawner,\n\
             {INDENT}{{\n"
        )
    } else {
        format!("{INDENT}pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self {{\n")
    };

    format!(
        "pub struct {name} {{\n\
         {fields}\
         }}\n\
         \n\
         impl {name} {{\n\
         {signature}\
         {bindings}\
         {INDENT}{INDENT}Self {{\n\
         {self_inits}\
         {INDENT}{INDENT}}}\n\
         {INDENT}}}\n\
         }}\n"
    )
}

fn needs_spawner(children: &[PinionChild]) -> bool {
    children.iter().any(|c| matches!(c, PinionChild::Resource(_)))
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
        bindings.push_str(&format!(
            "{INDENT}{INDENT}let {field} = \
             ::pinion_core::reactive::Computed::new(move || {{ {body} }});\n",
            field = c.name,
            body = c.body,
        ));
    } else {
        let (lhs, rhs) = capture_tuple(prior_names);
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

fn emit_resource_into(
    r: &ResourceDecl,
    prior_names: &[String],
    fields: &mut String,
    bindings: &mut String,
    inits: &mut String,
) {
    fields.push_str(&format!(
        "{INDENT}pub {field}: ::pinion_core::reactive::Resource<{ty}, {err}>,\n",
        field = r.name,
        ty = r.ty,
        err = r.err,
    ));

    // Initial state is Loading; the fetch_with call kicks off the
    // future immediately so the user's get() observes the eventual
    // Ready/Error transition.
    bindings.push_str(&format!(
        "{INDENT}{INDENT}let {field} = \
         ::pinion_core::reactive::Resource::<{ty}, {err}>::loading();\n",
        field = r.name,
        ty = r.ty,
        err = r.err,
    ));

    if prior_names.is_empty() {
        bindings.push_str(&format!(
            "{INDENT}{INDENT}{field}.fetch_with(spawner, {body});\n",
            field = r.name,
            body = r.body,
        ));
    } else {
        let (lhs, rhs) = capture_tuple(prior_names);
        bindings.push_str(&format!(
            "{INDENT}{INDENT}{{\n\
             {INDENT}{INDENT}{INDENT}#[allow(unused_variables, clippy::redundant_clone)]\n\
             {INDENT}{INDENT}{INDENT}let {lhs} = {rhs};\n\
             {INDENT}{INDENT}{INDENT}{field}.fetch_with(spawner, {body});\n\
             {INDENT}{INDENT}}}\n",
            field = r.name,
            body = r.body,
        ));
    }

    inits.push_str(&format!("{INDENT}{INDENT}{INDENT}{field},\n", field = r.name));
}

/// Build the over-capture `let` LHS and RHS as a parenthesized tuple.
/// Single-name uses the trailing-comma form (`(x,)`) for grammatical
/// uniformity with the multi-name case.
fn capture_tuple(prior_names: &[String]) -> (String, String) {
    debug_assert!(!prior_names.is_empty(), "capture_tuple called with no priors");
    if prior_names.len() == 1 {
        let n = &prior_names[0];
        (format!("({n},)"), format!("({n}.clone(),)"))
    } else {
        let lhs = format!("({})", prior_names.join(", "));
        let rhs = format!(
            "({})",
            prior_names.iter().map(|n| format!("{n}.clone()")).collect::<Vec<_>>().join(", ")
        );
        (lhs, rhs)
    }
}

fn child_name(child: &PinionChild) -> &str {
    match child {
        PinionChild::Signal(s) => &s.name,
        PinionChild::Computed(c) => &c.name,
        PinionChild::Resource(r) => &r.name,
    }
}
