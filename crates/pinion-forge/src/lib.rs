//! pinion-forge — pinion DSL (`.pinion.xml`) parser + Rust codegen per
//! §5.22 (R38) and §5.16 (R46).
//!
//! ## Scope
//!
//! pinion-forge consumes `.pinion.xml` files authored by humans or AI
//! agents and emits Rust source that targets the pinion-core runtime.
//! The root element is `<pinion xmlns="https://pinion.dev/dsl/v1"
//! kind="..." name="...">`; the kind selects the codegen template.
//!
//! - `kind="reactive"` (R38) compiles to one `pub struct <Name>` plus
//!   `impl <Name> { pub fn new(owner: &Owner) -> Self }` with a closed
//!   child set (`<use>` / `<signal>` / `<computed>` / `<resource>`).
//! - `kind="renderer"` (R46 §5.16) emits a backend manifest entry
//!   consumed by the build-time codegen template (commit 2 lands the
//!   Vello first emit template). The element is self-closing; its
//!   payload is the `backend` attribute.
//!
//! Per R37.7 + R37.8: pinion-forge owns its codegen. SCE upstream
//! rejects framework-specific kinds (RFC 001 closed); the only SCE
//! surface this crate consumes is the v1 NDJSON diagnostic *pattern*
//! (`v` / `id` / `code` / `stage` / `message` shape — see
//! `schemas/sce-diagnostic.v1.schema.json`). pinion's own diagnostic
//! namespace lives in [`diagnostic::PinionForgeDiagnostic`] and is not
//! an SCE `DiagnosticCode` extension.
//!
//! ## Kind dispatch
//!
//! [`PinionDoc`] carries a [`PinionSpec`] enum whose variants capture
//! the kind-specific payload (children for reactive, backend for
//! renderer). Codegen pattern-matches `PinionSpec` directly so adding a
//! new kind is a single `enum` variant addition — every existing
//! dispatch site flags up at compile time as a missing arm. The
//! parallel [`PinionKind`] enum exists for wire identity (the
//! `dsl/unknown-kind` diagnostic and any wire surface that needs the
//! kind as a string) and is not the dispatch axis.

#![forbid(unsafe_code)]

pub mod ast;
pub mod build;
pub mod codegen;
pub mod diagnostic;
pub mod parser;
pub mod wire;

pub use ast::{
    ComputedDecl, PinionChild, PinionDoc, PinionKind, PinionSpec, RendererBackend,
    RendererBackendKind, ResourceDecl, SignalDecl, UseDecl, VelloAaMode,
};
pub use build::{CompileError, compile_file, compile_str};
pub use codegen::emit_rust;
pub use diagnostic::{Location, PINION_DSL_NS, PinionForgeDiagnostic, Stage};
pub use parser::parse_pinion;
pub use wire::{WIRE_VERSION, to_json_value, to_ndjson_line};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn parse(xml: &str) -> Result<PinionDoc, Vec<PinionForgeDiagnostic>> {
        parse_pinion(xml, "<test>.pinion.xml")
    }

    fn parse_err(xml: &str) -> Vec<PinionForgeDiagnostic> {
        parse(xml).expect_err("expected diagnostic")
    }

    /// Test helper — destructure `doc.spec` as `PinionSpec::Reactive`,
    /// panicking with a clear message on a renderer doc. Keeps the test
    /// bodies free of inline `let-else` boilerplate.
    fn reactive_children(doc: &PinionDoc) -> &[PinionChild] {
        match &doc.spec {
            PinionSpec::Reactive { children } => children,
            PinionSpec::Renderer { .. } => {
                panic!("expected PinionSpec::Reactive, got Renderer")
            }
        }
    }

    #[test]
    fn parses_empty_reactive_self_closing() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty"/>"#;
        let doc = parse(xml).expect("happy path");
        assert_eq!(doc.name, "Empty");
        assert_eq!(doc.spec.kind(), PinionKind::Reactive);
        assert!(reactive_children(&doc).is_empty());
    }

    #[test]
    fn parses_empty_reactive_open_close_with_whitespace() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty">
        </pinion>"#;
        let doc = parse(xml).expect("happy path with whitespace body");
        assert_eq!(doc.name, "Empty");
    }

    #[test]
    fn emits_must_use_on_unit_struct_constructor() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="E"/>"#;
        let rust = compile_str(xml, "e.pinion.xml").expect("compile");
        // #[must_use] precedes the constructor — matches pinion-core
        // Signal/Computed/Resource convention for forwarding the
        // builder-style return type.
        let mu_pos = rust.find("#[must_use]").expect("must_use emitted");
        let fn_pos = rust.find("pub fn new").expect("fn emitted");
        assert!(mu_pos < fn_pos, "must_use must precede fn new");
    }

    #[test]
    fn emits_must_use_on_struct_with_signals() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="C">
            <signal name="x" ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let rust = compile_str(xml, "c.pinion.xml").expect("compile");
        assert!(rust.contains("#[must_use]\n    pub fn new("));
    }

    #[test]
    fn emits_must_use_on_struct_with_resources() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="R">
            <resource name="r" ty="i32" err="String"><![CDATA[async move { Ok(0) }]]></resource>
        </pinion>"#;
        let rust = compile_str(xml, "r.pinion.xml").expect("compile");
        // must_use precedes the generic-signature form
        assert!(rust.contains("#[must_use]\n    pub fn new<S>"));
    }

    #[test]
    fn emits_constructor_for_empty_doc() {
        let doc = PinionDoc {
            name: "Foo".into(),
            spec: PinionSpec::Reactive {
                children: Vec::new(),
            },
        };
        let rust = emit_rust(&doc);
        assert!(rust.contains("pub struct Foo;"));
        assert!(rust.contains("impl Foo"));
        assert!(rust.contains("pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self"));
        assert!(rust.contains("Self\n"));
        assert!(rust.ends_with('\n'));
    }

    #[test]
    fn roundtrip_parse_then_emit_compiles() {
        // The emitted source must syntactically parse as a Rust file. We
        // can't run rustc from a unit test, but `syn` is already in the
        // workspace via pinion-derive — fall back to a substring check
        // here and let the integration target (R38.2+ test crate)
        // exercise compilation end-to-end.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Button"/>"#;
        let rust = compile_str(xml, "ui/button.pinion.xml").expect("compile");
        assert!(rust.contains("pub struct Button;"));
    }

    #[test]
    fn rejects_non_pinion_root() {
        let xml = r#"<widget xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        assert_eq!(diags.len(), 1);
        let PinionForgeDiagnostic::InvalidRoot { found, .. } = &diags[0] else {
            panic!("expected InvalidRoot, got {:?}", diags[0]);
        };
        assert_eq!(found, "widget");
        assert_eq!(diags[0].code(), "dsl/invalid-root");
        assert_eq!(diags[0].stage(), Stage::Validate);
    }

    #[test]
    fn rejects_missing_xmlns() {
        let xml = r#"<pinion kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-xmlns"));
    }

    #[test]
    fn rejects_wrong_xmlns() {
        let xml = r#"<pinion xmlns="https://example.com/other" kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        let wrong = diags
            .iter()
            .find(|d| d.code() == "dsl/wrong-xmlns")
            .expect("wrong-xmlns");
        let PinionForgeDiagnostic::WrongXmlns { found, .. } = wrong else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "https://example.com/other");
    }

    #[test]
    fn rejects_missing_kind() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" name="X"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-kind"));
    }

    #[test]
    fn rejects_unknown_kind() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="view-fn" name="X"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unknown-kind")
            .expect("unknown-kind");
        let PinionForgeDiagnostic::UnknownKind { found, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "view-fn");
    }

    #[test]
    fn rejects_missing_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-name"));
    }

    #[test]
    fn rejects_invalid_name_keyword() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="impl"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/invalid-name")
            .expect("invalid-name");
        let PinionForgeDiagnostic::InvalidName { found, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "impl");
    }

    #[test]
    fn rejects_invalid_name_punctuation() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="foo-bar"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/invalid-name"));
    }

    #[test]
    fn rejects_invalid_name_leading_digit() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="1Foo"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/invalid-name"));
    }

    #[test]
    fn rejects_unsupported_child_element() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <effect name="e"/>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unsupported-element")
            .expect("unsupported-element");
        let PinionForgeDiagnostic::UnsupportedElement { tag, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "effect");
    }

    // ---- R38.2a: <signal> child element ----

    #[test]
    fn parses_single_signal() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Counter">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let doc = parse(xml).expect("happy path");
        let children = reactive_children(&doc);
        assert_eq!(children.len(), 1);
        let PinionChild::Signal(sig) = &children[0] else {
            panic!("expected Signal variant");
        };
        assert_eq!(sig.name, "count");
        assert_eq!(sig.ty, "i32");
        assert_eq!(sig.initial, "0");
    }

    #[test]
    fn parses_signal_with_plain_text_initial() {
        // CDATA is the textbook form, but plain Text is semantically
        // equivalent in XML — both must be accepted.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count" ty="i32">42</signal>
        </pinion>"#;
        let doc = parse(xml).expect("plain-text body");
        let PinionChild::Signal(sig) = &reactive_children(&doc)[0] else {
            panic!("expected Signal variant");
        };
        assert_eq!(sig.initial, "42");
    }

    #[test]
    fn parses_multiple_signals_preserves_order() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="State">
            <signal name="a" ty="i32"><![CDATA[1]]></signal>
            <signal name="b" ty="String"><![CDATA[String::new()]]></signal>
            <signal name="c" ty="bool"><![CDATA[true]]></signal>
        </pinion>"#;
        let doc = parse(xml).expect("multi-signal");
        let children = reactive_children(&doc);
        assert_eq!(children.len(), 3);
        let names: Vec<&str> = children
            .iter()
            .map(|c| match c {
                PinionChild::Signal(s) => s.name.as_str(),
                PinionChild::Computed(_) | PinionChild::Resource(_) | PinionChild::Use(_) => {
                    panic!("unexpected non-Signal variant")
                }
            })
            .collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }

    #[test]
    fn emits_struct_with_signal_fields() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Counter">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let rust = compile_str(xml, "counter.pinion.xml").expect("compile");
        assert!(rust.contains("pub struct Counter {"));
        assert!(rust.contains("pub count: ::pinion_core::reactive::Signal<i32>"));
        assert!(rust.contains("let count = ::pinion_core::reactive::Signal::new(0);"));
        // The unit-struct form must NOT appear when children are present.
        assert!(!rust.contains("pub struct Counter;"));
    }

    #[test]
    fn rejects_signal_missing_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/missing-attribute")
            .expect("missing-attribute");
        let PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "signal");
        assert_eq!(attribute, "name");
    }

    #[test]
    fn rejects_signal_missing_ty() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count"><![CDATA[0]]></signal>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| {
            matches!(d, PinionForgeDiagnostic::MissingAttribute { attribute, .. } if attribute == "ty")
        }));
    }

    #[test]
    fn rejects_signal_invalid_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="impl" ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/invalid-ident")
            .expect("invalid-ident");
        let PinionForgeDiagnostic::InvalidIdent {
            tag,
            attribute,
            found,
            ..
        } = bad
        else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "signal");
        assert_eq!(attribute, "name");
        assert_eq!(found, "impl");
    }

    #[test]
    fn rejects_signal_empty_body_self_closing() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count" ty="i32"/>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/empty-body")
            .expect("empty-body");
        let PinionForgeDiagnostic::EmptyBody { tag, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "signal");
    }

    #[test]
    fn rejects_signal_empty_body_whitespace_only() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count" ty="i32">
            </signal>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/empty-body"));
    }

    #[test]
    fn accumulates_signal_diagnostics_across_siblings() {
        // Three sibling <signal>s, each with a different problem. The
        // parser must surface all three without short-circuiting after
        // the first failure.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="ok" ty="i32"><![CDATA[0]]></signal>
            <signal ty="i32"><![CDATA[1]]></signal>
            <signal name="impl" ty="i32"><![CDATA[2]]></signal>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-attribute"));
        assert!(diags.iter().any(|d| d.code() == "dsl/invalid-ident"));
    }

    #[test]
    fn unsupported_sibling_does_not_block_signal_parsing() {
        // <effect> is unsupported at R38.2a; parser must still accept
        // the sibling <signal> rather than short-circuit. But because
        // <effect> emits a diagnostic, the overall result is Err — the
        // doc is unrenderable until the unsupported child is removed.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
            <effect name="e"/>
        </pinion>"#;
        let diags = parse_err(xml);
        // Both signal-parse-success and effect-unsupported diagnostics
        // are visible from a single run.
        assert!(diags.iter().any(|d| d.code() == "dsl/unsupported-element"));
        assert_eq!(
            diags
                .iter()
                .filter(|d| d.code() == "dsl/unsupported-element")
                .count(),
            1
        );
    }

    // ---- R38.2b: <computed> child element ----

    #[test]
    fn parses_single_computed() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Greeter">
            <computed name="msg" ty="String"><![CDATA[String::from("hi")]]></computed>
        </pinion>"#;
        let doc = parse(xml).expect("happy path");
        let children = reactive_children(&doc);
        assert_eq!(children.len(), 1);
        let PinionChild::Computed(c) = &children[0] else {
            panic!("expected Computed variant");
        };
        assert_eq!(c.name, "msg");
        assert_eq!(c.ty, "String");
        assert_eq!(c.body, r#"String::from("hi")"#);
    }

    #[test]
    fn parses_computed_referencing_signal_preserves_order() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Counter">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
            <computed name="doubled" ty="i32"><![CDATA[count.get() * 2]]></computed>
        </pinion>"#;
        let doc = parse(xml).expect("signal + computed");
        let children = reactive_children(&doc);
        assert_eq!(children.len(), 2);
        assert!(matches!(children[0], PinionChild::Signal(_)));
        assert!(matches!(children[1], PinionChild::Computed(_)));
    }

    #[test]
    fn emits_computed_with_over_capture() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Counter">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
            <computed name="doubled" ty="i32"><![CDATA[count.get() * 2]]></computed>
        </pinion>"#;
        let rust = compile_str(xml, "counter.pinion.xml").expect("compile");
        // Struct fields
        assert!(rust.contains("pub count: ::pinion_core::reactive::Signal<i32>"));
        assert!(rust.contains("pub doubled: ::pinion_core::reactive::Computed<i32>"));
        // Body bindings
        assert!(rust.contains("let count = ::pinion_core::reactive::Signal::new(0);"));
        assert!(rust.contains("let doubled = {"));
        assert!(rust.contains("#[allow(unused_variables, clippy::redundant_clone)]"));
        // Single-name tuple form
        assert!(rust.contains("let (count,) = (count.clone(),);"));
        // Closure wraps user body verbatim
        assert!(
            rust.contains("::pinion_core::reactive::Computed::new(move || { count.get() * 2 })")
        );
    }

    #[test]
    fn emits_computed_with_multi_capture_tuple() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="State">
            <signal name="a" ty="i32"><![CDATA[1]]></signal>
            <signal name="b" ty="i32"><![CDATA[2]]></signal>
            <computed name="sum" ty="i32"><![CDATA[a.get() + b.get()]]></computed>
        </pinion>"#;
        let rust = compile_str(xml, "state.pinion.xml").expect("compile");
        // Multi-element tuple uses no trailing comma
        assert!(rust.contains("let (a, b) = (a.clone(), b.clone());"));
    }

    #[test]
    fn emits_computed_with_no_priors_no_capture_block() {
        // A <computed> as the first child has no prior bindings to
        // capture — the plain closure form (no shadow block) is emitted.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="C">
            <computed name="forty_two" ty="i32"><![CDATA[42]]></computed>
        </pinion>"#;
        let rust = compile_str(xml, "c.pinion.xml").expect("compile");
        assert!(
            rust.contains(
                "let forty_two = ::pinion_core::reactive::Computed::new(move || { 42 });"
            )
        );
        // No shadow block when no priors
        assert!(!rust.contains("let (forty_two,)"));
        assert!(!rust.contains("#[allow(unused_variables"));
    }

    #[test]
    fn emits_chained_computed_captures_all_priors() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Chain">
            <signal name="a" ty="i32"><![CDATA[1]]></signal>
            <computed name="b" ty="i32"><![CDATA[a.get() + 1]]></computed>
            <computed name="c" ty="i32"><![CDATA[a.get() * b.get()]]></computed>
        </pinion>"#;
        let rust = compile_str(xml, "chain.pinion.xml").expect("compile");
        // `b` captures only `a`
        assert!(rust.contains("let b = {"));
        assert!(rust.contains("let (a,) = (a.clone(),);"));
        // `c` captures both `a` and `b`
        assert!(rust.contains("let c = {"));
        assert!(rust.contains("let (a, b) = (a.clone(), b.clone());"));
    }

    #[test]
    fn rejects_computed_missing_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <computed ty="i32"><![CDATA[0]]></computed>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/missing-attribute")
            .expect("missing-attribute");
        let PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "computed");
        assert_eq!(attribute, "name");
    }

    #[test]
    fn rejects_computed_missing_ty() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <computed name="c"><![CDATA[0]]></computed>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| matches!(
            d,
            PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. }
                if tag == "computed" && attribute == "ty"
        )));
    }

    #[test]
    fn rejects_computed_empty_body() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <computed name="c" ty="i32"/>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/empty-body")
            .expect("empty-body");
        let PinionForgeDiagnostic::EmptyBody { tag, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "computed");
    }

    #[test]
    fn rejects_computed_invalid_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <computed name="fn" ty="i32"><![CDATA[0]]></computed>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| matches!(
            d,
            PinionForgeDiagnostic::InvalidIdent { tag, attribute, .. }
                if tag == "computed" && attribute == "name"
        )));
    }

    // ---- R38.2d: <use> path import ----

    #[test]
    fn parses_single_use() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <use path="my_crate::widgets::Button"/>
        </pinion>"#;
        let doc = parse(xml).expect("happy path");
        let children = reactive_children(&doc);
        assert_eq!(children.len(), 1);
        let PinionChild::Use(u) = &children[0] else {
            panic!("expected Use variant");
        };
        assert_eq!(u.path, "my_crate::widgets::Button");
    }

    #[test]
    fn parses_use_open_close_form_ignores_body() {
        // <use path="..."> body </use> — body is silently skipped per
        // R38.2d. The parser succeeds without diagnostic.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <use path="foo::Bar">whatever junk in body</use>
        </pinion>"#;
        let doc = parse(xml).expect("body silently ignored");
        let PinionChild::Use(u) = &reactive_children(&doc)[0] else {
            panic!("expected Use variant");
        };
        assert_eq!(u.path, "foo::Bar");
    }

    #[test]
    fn emits_use_at_top_of_file() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Imports">
            <use path="my_crate::widgets::Button"/>
        </pinion>"#;
        let rust = compile_str(xml, "imports.pinion.xml").expect("compile");
        // `use` statement must come before `pub struct`.
        let use_pos = rust
            .find("use my_crate::widgets::Button;")
            .expect("use emitted");
        let struct_pos = rust.find("pub struct Imports").expect("struct emitted");
        assert!(use_pos < struct_pos, "use must precede struct");
    }

    #[test]
    fn emits_multiple_use_statements_in_order() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Multi">
            <use path="foo::A"/>
            <use path="bar::{B, C}"/>
            <use path="baz::D as RenamedD"/>
        </pinion>"#;
        let rust = compile_str(xml, "multi.pinion.xml").expect("compile");
        let a = rust.find("use foo::A;").expect("A");
        let b = rust.find("use bar::{B, C};").expect("B,C group");
        let d = rust.find("use baz::D as RenamedD;").expect("rename");
        assert!(a < b && b < d, "order preserved");
    }

    #[test]
    fn use_alone_emits_unit_struct() {
        // <use> children alone do not produce a binding — the struct
        // collapses to unit form.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="OnlyImports">
            <use path="my_crate::Foo"/>
        </pinion>"#;
        let rust = compile_str(xml, "onlyimports.pinion.xml").expect("compile");
        assert!(rust.contains("use my_crate::Foo;"));
        assert!(rust.contains("pub struct OnlyImports;"));
        // unit-struct constructor returns plain `Self`
        assert!(rust.contains("Self\n"));
    }

    #[test]
    fn use_does_not_contribute_to_prior_names() {
        // <use> before a <computed> must NOT appear in the capture
        // tuple — module-level imports are visible without capture.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
            <use path="std::convert::From"/>
            <computed name="doubled" ty="i32"><![CDATA[count.get() * 2]]></computed>
        </pinion>"#;
        let rust = compile_str(xml, "x.pinion.xml").expect("compile");
        // Capture tuple must contain only `count`, not the use path.
        assert!(rust.contains("let (count,) = (count.clone(),);"));
        assert!(!rust.contains("From,)"));
        assert!(!rust.contains("From.clone()"));
    }

    #[test]
    fn rejects_use_missing_path() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <use/>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/missing-attribute")
            .expect("missing-attribute");
        let PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "use");
        assert_eq!(attribute, "path");
    }

    #[test]
    fn rejects_use_empty_path() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <use path=""/>
        </pinion>"#;
        let diags = parse_err(xml);
        // Empty after trim is treated as missing per require_nonempty_attr.
        assert!(diags.iter().any(|d| matches!(
            d,
            PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. }
                if tag == "use" && attribute == "path"
        )));
    }

    // ---- R38.2c: <resource> child element ----

    #[test]
    fn parses_single_resource() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="UserView">
            <resource name="user" ty="String" err="String">
                <![CDATA[async move { Ok::<String, String>("hi".into()) }]]>
            </resource>
        </pinion>"#;
        let doc = parse(xml).expect("happy path");
        let children = reactive_children(&doc);
        assert_eq!(children.len(), 1);
        let PinionChild::Resource(r) = &children[0] else {
            panic!("expected Resource variant");
        };
        assert_eq!(r.name, "user");
        assert_eq!(r.ty, "String");
        assert_eq!(r.err, "String");
        assert!(r.body.contains("async move"));
    }

    #[test]
    fn emits_resource_struct_field_and_loading_init() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="U">
            <resource name="user" ty="String" err="ApiError">
                <![CDATA[async move { fetch_user().await }]]>
            </resource>
        </pinion>"#;
        let rust = compile_str(xml, "u.pinion.xml").expect("compile");
        assert!(rust.contains("pub user: ::pinion_core::reactive::Resource<String, ApiError>"));
        assert!(rust.contains(
            "let user = ::pinion_core::reactive::Resource::<String, ApiError>::loading();"
        ));
        assert!(rust.contains("user.fetch_with(spawner, async move { fetch_user().await });"));
    }

    #[test]
    fn emits_resource_signature_takes_generic_spawner() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="U">
            <resource name="r" ty="i32" err="String">
                <![CDATA[async move { Ok(0) }]]>
            </resource>
        </pinion>"#;
        let rust = compile_str(xml, "u.pinion.xml").expect("compile");
        // Signature must mention the generic + bound + spawner parameter.
        assert!(rust.contains(
            "pub fn new<S>(_owner: &::pinion_core::reactive::Owner, spawner: &S) -> Self"
        ));
        assert!(rust.contains("S: ::pinion_core::reactive::LocalSpawner"));
    }

    #[test]
    fn empty_doc_signature_does_not_take_spawner() {
        // Regression: a doc with no <resource> child must keep the
        // one-argument new() signature (R38.1 shape).
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty"/>"#;
        let rust = compile_str(xml, "e.pinion.xml").expect("compile");
        assert!(rust.contains("pub fn new(_owner: &::pinion_core::reactive::Owner) -> Self"));
        assert!(!rust.contains("spawner"));
        assert!(!rust.contains("LocalSpawner"));
    }

    #[test]
    fn signal_only_signature_does_not_take_spawner() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="C">
            <signal name="count" ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let rust = compile_str(xml, "c.pinion.xml").expect("compile");
        assert!(!rust.contains("spawner"));
        assert!(!rust.contains("LocalSpawner"));
    }

    #[test]
    fn resource_with_prior_signal_emits_over_capture_block() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="View">
            <signal name="id" ty="u64"><![CDATA[1]]></signal>
            <resource name="data" ty="String" err="String">
                <![CDATA[async move { Ok::<String, String>(id.get().to_string()) }]]>
            </resource>
        </pinion>"#;
        let rust = compile_str(xml, "v.pinion.xml").expect("compile");
        // The shadow block for the resource fetch must capture `id`.
        assert!(rust.contains("#[allow(unused_variables, clippy::redundant_clone)]"));
        assert!(rust.contains("let (id,) = (id.clone(),);"));
        assert!(rust.contains("data.fetch_with(spawner,"));
    }

    #[test]
    fn rejects_resource_missing_err() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <resource name="r" ty="i32"><![CDATA[async move { Ok(0) }]]></resource>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| matches!(
            d,
            PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. }
                if tag == "resource" && attribute == "err"
        )));
    }

    #[test]
    fn rejects_resource_missing_name() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <resource ty="i32" err="String"><![CDATA[async move { Ok(0) }]]></resource>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| matches!(
            d,
            PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. }
                if tag == "resource" && attribute == "name"
        )));
    }

    #[test]
    fn rejects_resource_empty_body() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <resource name="r" ty="i32" err="String"/>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/empty-body")
            .expect("empty-body");
        let PinionForgeDiagnostic::EmptyBody { tag, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "resource");
    }

    #[test]
    fn accumulates_diagnostics_across_all_three_child_types() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal ty="i32"><![CDATA[0]]></signal>
            <computed name="impl" ty="i32"><![CDATA[0]]></computed>
            <resource name="r" ty="i32"><![CDATA[async move { Ok(0) }]]></resource>
        </pinion>"#;
        let diags = parse_err(xml);
        let by_kind: std::collections::BTreeSet<_> = diags
            .iter()
            .filter_map(|d| match d {
                PinionForgeDiagnostic::MissingAttribute { tag, attribute, .. } => {
                    Some(("missing", tag.as_str(), attribute.as_str()))
                }
                PinionForgeDiagnostic::InvalidIdent { tag, attribute, .. } => {
                    Some(("invalid", tag.as_str(), attribute.as_str()))
                }
                _ => None,
            })
            .collect();
        assert!(by_kind.contains(&("missing", "signal", "name")));
        assert!(by_kind.contains(&("invalid", "computed", "name")));
        assert!(by_kind.contains(&("missing", "resource", "err")));
    }

    #[test]
    fn accumulates_signal_and_computed_diagnostics_in_one_run() {
        // A doc with one bad <signal> and one bad <computed> must
        // surface both diagnostics in a single pass.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X">
            <signal ty="i32"><![CDATA[0]]></signal>
            <computed name="impl" ty="i32"><![CDATA[0]]></computed>
        </pinion>"#;
        let diags = parse_err(xml);
        let by_tag: std::collections::BTreeSet<_> = diags
            .iter()
            .filter_map(|d| match d {
                PinionForgeDiagnostic::MissingAttribute { tag, .. } => {
                    Some(("missing", tag.as_str()))
                }
                PinionForgeDiagnostic::InvalidIdent { tag, .. } => Some(("invalid", tag.as_str())),
                _ => None,
            })
            .collect();
        assert!(by_tag.contains(&("missing", "signal")));
        assert!(by_tag.contains(&("invalid", "computed")));
    }

    #[test]
    fn wire_record_has_tag_and_attribute_in_id_for_missing_attribute() {
        // Two MissingAttribute diagnostics on the same (tag, location)
        // but different attributes must hash to different ids.
        let loc = Location::new("a.pinion.xml");
        let a = PinionForgeDiagnostic::MissingAttribute {
            tag: "signal".into(),
            attribute: "name".into(),
            location: loc.clone(),
        };
        let b = PinionForgeDiagnostic::MissingAttribute {
            tag: "signal".into(),
            attribute: "ty".into(),
            location: loc,
        };
        assert_ne!(to_json_value(&a)["id"], to_json_value(&b)["id"]);
    }

    #[test]
    fn accumulates_multiple_attribute_diagnostics() {
        // Missing all three required attributes — parser must surface
        // each independently rather than fail-fast on the first.
        let xml = r"<pinion/>";
        let diags = parse_err(xml);
        let codes: std::collections::BTreeSet<_> =
            diags.iter().map(PinionForgeDiagnostic::code).collect();
        assert!(codes.contains("dsl/missing-xmlns"));
        assert!(codes.contains("dsl/missing-kind"));
        assert!(codes.contains("dsl/missing-name"));
    }

    #[test]
    fn rejects_malformed_xml() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="X""#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/xml-parse"));
        assert!(diags.iter().any(|d| d.stage() == Stage::Parse));
    }

    #[test]
    fn wire_record_has_required_fields() {
        let xml = r#"<pinion kind="reactive" name="X"/>"#;
        let diags = parse_err(xml);
        let diag = diags
            .iter()
            .find(|d| d.code() == "dsl/missing-xmlns")
            .unwrap();
        let line = to_ndjson_line(diag);
        let value: Value = serde_json::from_str(&line).expect("valid JSON");
        let obj = value.as_object().expect("object");
        assert_eq!(obj.get("v"), Some(&Value::from(1)));
        assert!(
            obj.get("id")
                .and_then(Value::as_str)
                .unwrap()
                .starts_with("fnv1a:")
        );
        assert_eq!(obj.get("code"), Some(&Value::from("dsl/missing-xmlns")));
        assert_eq!(obj.get("stage"), Some(&Value::from("validate")));
        assert!(obj.get("message").is_some());
        let loc = obj
            .get("location")
            .and_then(Value::as_object)
            .expect("location");
        assert!(loc.get("file").is_some());
    }

    #[test]
    fn wire_id_is_stable_under_message_rewording() {
        // The (code, stage, file, key_fragments) tuple defines identity.
        // Two diagnostics built independently with the same identity
        // fields must produce the same `id`, even if (hypothetically) we
        // reworded the Display message.
        let a = PinionForgeDiagnostic::UnsupportedElement {
            tag: "signal".into(),
            location: Location::new("foo.pinion.xml").with_line_col(2, 13),
        };
        let b = PinionForgeDiagnostic::UnsupportedElement {
            tag: "signal".into(),
            location: Location::new("foo.pinion.xml").with_line_col(99, 42),
        };
        let va = to_json_value(&a);
        let vb = to_json_value(&b);
        assert_eq!(va["id"], vb["id"], "id must not depend on line/col");
    }

    #[test]
    fn wire_id_differs_across_codes() {
        let a = PinionForgeDiagnostic::MissingXmlns {
            expected: PINION_DSL_NS,
            location: Location::new("a.pinion.xml"),
        };
        let b = PinionForgeDiagnostic::MissingName {
            location: Location::new("a.pinion.xml"),
        };
        let va = to_json_value(&a);
        let vb = to_json_value(&b);
        assert_ne!(va["id"], vb["id"]);
    }

    // ---- R46 §5.16: <pinion kind="renderer"> ----

    #[test]
    fn parses_empty_renderer_self_closing() {
        // R46.1 manifest shape (no `aa` attribute) → defaults to Area
        // per R46.2.1 forward-compat.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello"/>"#;
        let doc = parse(xml).expect("happy path");
        assert_eq!(doc.name, "Scene");
        assert_eq!(doc.spec.kind(), PinionKind::Renderer);
        let PinionSpec::Renderer { backend } = doc.spec else {
            panic!("expected Renderer variant");
        };
        assert_eq!(
            backend,
            RendererBackend::Vello {
                aa: VelloAaMode::Area
            }
        );
        assert_eq!(backend.kind(), RendererBackendKind::Vello);
    }

    #[test]
    fn parses_renderer_open_close_form_without_children() {
        // Renderer accepts the open-close form as long as the body
        // contains no child elements (whitespace only is fine).
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello">
        </pinion>"#;
        let doc = parse(xml).expect("renderer open-close body");
        assert_eq!(doc.name, "Scene");
        assert!(matches!(
            doc.spec,
            PinionSpec::Renderer {
                backend: RendererBackend::Vello {
                    aa: VelloAaMode::Area
                }
            }
        ));
    }

    #[test]
    fn rejects_renderer_missing_backend() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/missing-backend")
            .expect("missing-backend");
        assert!(matches!(bad, PinionForgeDiagnostic::MissingBackend { .. }));
        assert_eq!(bad.stage(), Stage::Validate);
    }

    #[test]
    fn rejects_renderer_unknown_backend() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="wgpu"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unknown-backend")
            .expect("unknown-backend");
        let PinionForgeDiagnostic::UnknownBackend { found, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "wgpu");
        assert_eq!(bad.stage(), Stage::Validate);
    }

    #[test]
    fn rejects_renderer_empty_backend_treated_as_missing() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend=""/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/missing-backend"));
    }

    #[test]
    fn rejects_renderer_with_child_element() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello">
            <signal name="x" ty="i32"><![CDATA[0]]></signal>
        </pinion>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/renderer-child-not-allowed")
            .expect("renderer-child-not-allowed");
        let PinionForgeDiagnostic::RendererChildNotAllowed { tag, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(tag, "signal");
        assert_eq!(bad.stage(), Stage::Validate);
    }

    #[test]
    fn renderer_child_diagnostic_uses_renderer_code_not_unsupported() {
        // The reactive grammar emits `dsl/unsupported-element` for unknown
        // children; the renderer grammar emits `dsl/renderer-child-not-allowed`
        // because the failure mode is different (any child is wrong vs
        // this specific child is wrong). Authoring guidance follows the
        // kind, so the codes are distinct.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello">
            <effect name="e"/>
        </pinion>"#;
        let diags = parse_err(xml);
        assert!(
            diags
                .iter()
                .any(|d| d.code() == "dsl/renderer-child-not-allowed")
        );
        assert!(!diags.iter().any(|d| d.code() == "dsl/unsupported-element"));
    }

    #[test]
    fn emits_renderer_vello_struct_and_constructor_signature() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="MainScene" backend="vello"/>"#;
        let rust = compile_str(xml, "scene.pinion.xml").expect("compile");
        // R46.2: the renderer kind emits a concrete Rust type wrapping
        // vello::Renderer + the owned GPU device/surface (no virtual
        // dispatch). R46.3.3: all Vello types reference fully-qualified
        // `::vello::*` paths so include!() is namespace-safe at any
        // consumer scope. R1537 moved the device out of vello's
        // `RenderContext` — see
        // `emits_renderer_vello_uses_canonical_vello_api_surface`.
        assert!(rust.contains("pub struct MainScene {"));
        assert!(rust.contains("context: ::pinion_gpu::GpuContext"));
        assert!(rust.contains("surface: ::pinion_gpu::GpuSurface"));
        assert!(rust.contains("renderer: ::vello::Renderer"));
        assert!(rust.contains("frame_timer: ::std::option::Option<::pinion_gpu::FrameTimer>"));
        // async new<W: Into<::vello::wgpu::SurfaceTarget<'static>>>(...) -> Result<Self, MainSceneError>
        assert!(rust.contains("pub async fn new<W>(target: W, width: u32, height: u32) -> ::std::result::Result<Self, MainSceneError>"));
        assert!(rust.contains("W: ::std::convert::Into<::vello::wgpu::SurfaceTarget<'static>>"));
        // sync render(scene, base_color) and resize(w, h)
        assert!(rust.contains("pub fn render(\n        &mut self,\n        scene: &::vello::Scene,\n        base_color: ::vello::peniko::Color,\n    ) -> ::std::result::Result<(), MainSceneError>"));
        assert!(rust.contains("pub fn resize(&mut self, width: u32, height: u32)"));
        // Stub markers absent
        assert!(!rust.contains("renderer kind codegen stub"));
        assert!(!rust.contains("unimplemented!"));
    }

    #[test]
    fn emits_renderer_vello_error_enum_with_from_impls() {
        // The emitted module defines a closed error enum named
        // `<Name>Error` carrying `::vello::Error` (with a `From`
        // conversion so `?` propagation works for renderer init + frame
        // submission) and a labelled `Surface(&'static str)` variant for
        // the wgpu 29 `CurrentSurfaceTexture` non-success states (no
        // `From`/`?` — the status enum is matched directly in `render`).
        // R46.3.3 fully-qualified paths.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Demo" backend="vello"/>"#;
        let rust = compile_str(xml, "demo.pinion.xml").expect("compile");
        assert!(rust.contains("pub enum DemoError {"));
        assert!(rust.contains("Vello(::vello::Error),"));
        assert!(rust.contains("Surface(&'static str),"));
        // R1537 — device/surface establishment fails before a rasterizer
        // exists, so it cannot be reported as a vello error.
        assert!(rust.contains("Gpu(::pinion_gpu::GpuError),"));
        assert!(rust.contains("impl ::std::convert::From<::pinion_gpu::GpuError> for DemoError"));
        // std::error::Error + Display impls (fully-qualified)
        assert!(rust.contains("impl ::std::fmt::Display for DemoError"));
        assert!(rust.contains("impl ::std::error::Error for DemoError"));
        // From impl for ? propagation (vello::Error only; surface
        // acquisition is a status-enum match, not a `?`-able Result).
        assert!(rust.contains("impl ::std::convert::From<::vello::Error> for DemoError"));
        assert!(!rust.contains("From<::vello::wgpu::SurfaceError>"));
        // wgpu 29 surface acquisition match (status enum, not Result).
        assert!(rust.contains("::vello::wgpu::CurrentSurfaceTexture::Success(t)"));
    }

    #[test]
    fn emits_renderer_vello_uses_canonical_vello_api_surface() {
        // The template must use Vello 0.9's canonical *rendering* API —
        // render_to_texture into an intermediate target, blit, present —
        // and not a hand-rolled compute pipeline. R46.3.3: fully-qualified
        // paths (no `use vello::*` items in the emitted file).
        //
        // R1537 — the canonical *device* helper (`vello::util::RenderContext`)
        // is deliberately NOT used. Its `new_device` is private and requests
        // a fixed feature set, so a device it creates can never carry
        // TIMESTAMP_QUERY and no frame drawn on it can state what the GPU
        // took. `vello::Renderer::new` takes a `&Device` the caller owns,
        // which is the seam this template uses instead.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello"/>"#;
        let rust = compile_str(xml, "x.pinion.xml").expect("compile");
        // No `use vello::*` items — R46.3.3 namespace contract.
        assert!(
            !rust.contains("use vello::"),
            "R46.3.3: no `use vello::*` imports in emitted code"
        );
        // Vello 0.9 canonical pattern markers (fully-qualified)
        assert!(
            !rust.contains("::vello::util::RenderContext"),
            "R1537: the emitted renderer must own its device, not delegate \
             to vello's fixed-feature RenderContext"
        );
        assert!(rust.contains("::pinion_gpu::GpuContext::new("));
        assert!(rust.contains("render_to_texture("));
        assert!(rust.contains("self.surface.blit("));
        assert!(rust.contains("surface_texture.present();"));
        // AutoVsync (textbook default, fully-qualified)
        assert!(rust.contains("::vello::wgpu::PresentMode::AutoVsync"));
        // R46.2.1 + R46.3.3: AaSupport struct literal matches AaConfig variant
        // (default = Area). Both placeholders fully-qualified.
        assert!(rust.contains(
            "antialiasing_support: ::vello::AaSupport { area: true, msaa8: false, msaa16: false }"
        ));
        assert!(rust.contains("antialiasing_method: ::vello::AaConfig::Area"));
    }

    #[test]
    fn emits_renderer_vello_times_the_acquire_and_cannot_report_it_stale() {
        // R1361.1/.4 §5.16 — the acquire block must be measured SEPARATELY
        // from render work. `render_us` used to contain
        // `get_current_texture()`, which blocks on vsync under AutoVsync, so
        // a window doing 0.4ms of drawing reported 16ms of "render" and read
        // exactly like a GPU-bound one.
        //
        // Two properties, and the second is the subtle one:
        //
        //  1. the acquire is timed and stored before the non-success arms
        //     diverge (a timeout IS the block worth reporting);
        //  2. the field is CLEARED at the top of `render`, because
        //     `render_to_texture(...)?` returns early — before the acquire
        //     is ever attempted. Without the reset, a failed frame keeps the
        //     PREVIOUS frame's block, and the shell subtracts a wait that
        //     never happened: that frame is then recorded as ~all acquire and
        //     0 render. A frame that never reached the acquire blocked 0µs.
        //
        // A source-text test because the property lives in generated code on
        // a GPU path no unit test can drive.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Timed" backend="vello"/>"#;
        let rust = compile_str(xml, "timed.pinion.xml").expect("compile");

        let render_body = rust
            .split_once("pub fn render(")
            .expect("template emits an inherent render")
            .1;
        let reset_at = render_body.find("self.last_acquire_us = 0;").expect(
            "render CLEARS last_acquire_us — a stale block must never survive an early return",
        );
        let work_at = render_body
            .find("render_to_texture(")
            .expect("render calls render_to_texture");
        assert!(
            reset_at < work_at,
            "the reset must precede `render_to_texture`, whose `?` returns \
             early: reset at {reset_at}, render_to_texture at {work_at}",
        );

        // The acquire is bracketed, and stored before the match arms.
        let start_at = render_body
            .find("let __acquire_start = ::std::time::Instant::now();")
            .expect("the acquire span is opened");
        let store_at = render_body
            .find("self.last_acquire_us = ::std::convert::TryFrom::try_from(")
            .expect("the acquire span is stored");
        let match_at = render_body
            .find("match __acquired {")
            .expect("the acquire result is matched after being timed");
        assert!(
            start_at < store_at && store_at < match_at,
            "the acquire must be timed and stored BEFORE the arms diverge, \
             so a Timeout frame still reports its block: start={start_at} \
             store={store_at} match={match_at}",
        );
        // The block must be measured around the acquire ALONE — if the call
        // moved back inside the match scrutinee the span would be lost.
        assert!(
            render_body.contains("let __acquired = self.surface.acquire();"),
            "the acquire is called on its own line inside the timed span",
        );
        // And the accessor the shell reads it through exists.
        assert!(
            rust.contains("pub fn last_acquire_us(&self) -> u64 {"),
            "the template exposes the block so the shell can subtract it",
        );
    }

    #[test]
    fn emits_renderer_vello_brackets_the_frame_with_gpu_timestamps_in_order() {
        // R1537 §5.16 — `render_us` is CPU submit cost: `wgpu` returns from
        // `submit` before the GPU has run anything, so a fully GPU-bound
        // window reports fast CPU phases and nothing on the wire disagrees.
        // The template now brackets the frame with two timestamp queries.
        //
        // ORDER is the whole contract, and it is what this asserts. A pair
        // of timestamps that compiles but sits in the wrong places still
        // produces a plausible number — one that measures the blit alone,
        // or spans two frames — and no type can catch that. Specifically:
        //
        //   open  < render_to_texture   the opening stamp must be SUBMITTED
        //                               before the rasterizer's own internal
        //                               submit, or the compute passes fall
        //                               outside the span;
        //   blit  < end                 the closing stamp rides the blit
        //                               encoder, after the blit is recorded;
        //   end   < submit < after_submit
        //                               a map can only be asked for once the
        //                               commands it waits on are submitted.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="T" backend="vello"/>"#;
        let rust = compile_str(xml, "t.pinion.xml").expect("compile");
        let body_at = rust.find("pub fn render(").expect("render fn");
        let body = &rust[body_at..];
        let at = |needle: &str| -> usize {
            body.find(needle)
                .unwrap_or_else(|| panic!("render body is missing `{needle}`"))
        };

        let collect_at = at("timer.collect(self.context.device());");
        let begin_at = at("if timer.begin(&mut open) {");
        let open_submit_at = at("self.context.queue().submit([open.finish()]);");
        let raster_at = at("self.renderer.render_to_texture(");
        let blit_at = at("self.surface.blit(");
        let end_at = at("timer.end(&mut encoder);");
        let submit_at = at("self.context.queue().submit([encoder.finish()]);");
        let after_at = at("timer.after_submit();");
        let present_at = at("surface_texture.present();");

        assert!(
            collect_at < begin_at,
            "harvest the previous measurement before starting a new one, or \
             the timer never frees its single slot: collect={collect_at} \
             begin={begin_at}",
        );
        assert!(
            begin_at < open_submit_at && open_submit_at < raster_at,
            "the opening timestamp must be SUBMITTED before the rasterizer \
             submits its own work, or the compute passes fall outside the \
             measured span: begin={begin_at} submit={open_submit_at} \
             raster={raster_at}",
        );
        assert!(
            blit_at < end_at && end_at < submit_at,
            "the closing timestamp rides the blit encoder, recorded after \
             the blit and before that encoder is finished: blit={blit_at} \
             end={end_at} submit={submit_at}",
        );
        assert!(
            submit_at < after_at && after_at < present_at,
            "the staging map is asked for only after the commands it waits \
             on are submitted: submit={submit_at} after={after_at} \
             present={present_at}",
        );

        // The accessors the shell reads the clock through.
        assert!(rust.contains("pub fn gpu_clock(&mut self) -> ::pinion_gpu::GpuFrameClock {"));
        assert!(rust.contains("pub fn gpu_timing_counts(&self) -> (u64, u64) {"));
        // The unsupported case is the ABSENT timer, mapped explicitly —
        // not a zero, and not a panic.
        assert!(rust.contains("::pinion_gpu::GpuFrameClock::Unsupported"));
    }

    #[test]
    fn emits_renderer_vello_pays_nothing_for_timing_it_cannot_do() {
        // R1537 — the extra command buffer is the entire cost of the
        // measurement, and a host without timestamp queries must not pay
        // it. `frame_timer` is `None` there, so every timing site sits
        // inside an `if let Some(timer)` and the encoder is never created.
        //
        // Asserted on the emitted text because there is no other way to
        // observe it: on a host that DOES support timestamps the branch is
        // always taken, so a test that runs the code cannot distinguish
        // "gated" from "unconditional".
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="G" backend="vello"/>"#;
        let rust = compile_str(xml, "g.pinion.xml").expect("compile");
        let body = &rust[rust.find("pub fn render(").expect("render fn")..];
        let gate = "if let ::std::option::Option::Some(timer) = self.frame_timer.as_mut() {";
        assert_eq!(
            body.matches(gate).count(),
            3,
            "every timing site — open, close, map — is gated on the timer \
             existing, so a host that cannot time the GPU creates no \
             encoder and submits no extra command buffer",
        );
        // And the field really can be absent: the template must not
        // `unwrap` or `expect` its way past the option.
        assert!(
            !body.contains("frame_timer.as_mut().unwrap()")
                && !body.contains("frame_timer.as_mut().expect("),
            "a host without timestamp queries is a supported host, not a panic",
        );
    }

    #[test]
    fn emits_renderer_vello_recovers_invalidated_surface() {
        // R1049 §5.16 (PR-21) — a non-presentable surface status
        // (outdated / lost / validation) must RECONFIGURE the swapchain,
        // not merely skip the frame: a surface stays broken until
        // reconfigured, and a stale-but-"Success" texture then panics the
        // process at `create_view`. The device also installs an
        // uncaptured-error handler so a transient validation error is
        // absorbed rather than hard-panicking via wgpu's default.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello"/>"#;
        let rust = compile_str(xml, "scene.pinion.xml").expect("compile");
        // Exactly the three recoverable statuses reconfigure (Timeout /
        // Occluded are not surface losses → they skip without reconfigure).
        assert_eq!(
            rust.matches("self.context.configure_surface(&self.surface);")
                .count(),
            3,
            "outdated / lost / validation each reconfigure the surface",
        );
        // ...and still surface a labelled error so the shell logs + skips
        // THIS frame (recovery lands on the next acquisition).
        for reason in ["outdated", "lost", "validation"] {
            assert!(
                rust.contains(&format!("Surface(\"{reason}\")")),
                "recoverable status {reason} still returns its labelled Surface error",
            );
        }
        // Uncaptured-error handler installed at device creation (wgpu 29
        // takes an Arc).
        assert!(
            rust.contains("context.on_uncaptured_error(|error| {"),
            "device installs an uncaptured-error handler to absorb transient validation errors",
        );
    }

    // ---- R46.2.1 §5.16: aa manifest attribute (Vello AA mode) ----

    #[test]
    fn parses_renderer_aa_msaa8_attribute() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello" aa="msaa8"/>"#;
        let doc = parse(xml).expect("happy path");
        let PinionSpec::Renderer { backend } = doc.spec else {
            panic!("expected Renderer variant");
        };
        assert_eq!(
            backend,
            RendererBackend::Vello {
                aa: VelloAaMode::Msaa8
            }
        );
    }

    #[test]
    fn parses_renderer_aa_msaa16_attribute() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello" aa="msaa16"/>"#;
        let doc = parse(xml).expect("happy path");
        let PinionSpec::Renderer { backend } = doc.spec else {
            panic!("expected Renderer variant");
        };
        assert_eq!(
            backend,
            RendererBackend::Vello {
                aa: VelloAaMode::Msaa16
            }
        );
    }

    #[test]
    fn parses_renderer_aa_area_attribute_explicit() {
        // Explicitly opting into the default — must accept and produce
        // the same AST as no-attribute case.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello" aa="area"/>"#;
        let doc = parse(xml).expect("happy path");
        let PinionSpec::Renderer { backend } = doc.spec else {
            panic!("expected Renderer variant");
        };
        assert_eq!(
            backend,
            RendererBackend::Vello {
                aa: VelloAaMode::Area
            }
        );
    }

    #[test]
    fn defaults_renderer_aa_to_area_when_absent() {
        // R46.1 manifest shape (no `aa` attribute) must keep working —
        // backward-compat with all R46.1-vintage manifests.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello"/>"#;
        let doc = parse(xml).expect("R46.1 shape backward-compat");
        let PinionSpec::Renderer { backend } = doc.spec else {
            panic!("expected Renderer variant");
        };
        assert_eq!(
            backend,
            RendererBackend::Vello {
                aa: VelloAaMode::Area
            }
        );
    }

    #[test]
    fn defaults_renderer_aa_to_area_when_whitespace_only() {
        // Whitespace-only `aa` treated as absent (matches MissingBackend
        // policy for whitespace-only `backend`) — defaults to Area
        // rather than raising UnknownAa.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello" aa="   "/>"#;
        let doc = parse(xml).expect("whitespace aa treated as absent");
        let PinionSpec::Renderer { backend } = doc.spec else {
            panic!("expected Renderer variant");
        };
        assert_eq!(
            backend,
            RendererBackend::Vello {
                aa: VelloAaMode::Area
            }
        );
    }

    #[test]
    fn rejects_renderer_unknown_aa() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Scene" backend="vello" aa="fxaa"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unknown-aa")
            .expect("unknown-aa");
        let PinionForgeDiagnostic::UnknownAa { found, .. } = bad else {
            panic!("variant mismatch");
        };
        assert_eq!(found, "fxaa");
        assert_eq!(bad.stage(), Stage::Validate);
    }

    #[test]
    fn renderer_wire_diagnostic_carries_aa_actual() {
        // Like UnknownBackend, the wire `actual` field surfaces the
        // offending literal so an agent can repair it without re-parsing
        // the message text.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello" aa="taa"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unknown-aa")
            .expect("unknown-aa");
        let value = to_json_value(bad);
        assert_eq!(value["actual"], Value::from("taa"));
    }

    #[test]
    fn aa_unknown_emits_only_aa_diagnostic_when_backend_is_valid() {
        // Backend valid + aa unknown → only UnknownAa, not UnknownBackend.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello" aa="unknown"/>"#;
        let diags = parse_err(xml);
        assert!(diags.iter().any(|d| d.code() == "dsl/unknown-aa"));
        assert!(!diags.iter().any(|d| d.code() == "dsl/unknown-backend"));
    }

    #[test]
    fn emits_renderer_vello_aa_msaa16_substitutes_struct_literal() {
        // Manifest aa="msaa16" must produce the matching AaSupport
        // struct literal (only msaa16 = true) AND the matching
        // AaConfig::Msaa16 runtime method. The pair must agree —
        // Vello panics if RenderParams method is outside support set.
        // R46.3.3 fully-qualified paths.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello" aa="msaa16"/>"#;
        let rust = compile_str(xml, "x.pinion.xml").expect("compile");
        assert!(rust.contains(
            "antialiasing_support: ::vello::AaSupport { area: false, msaa8: false, msaa16: true }"
        ));
        assert!(rust.contains("antialiasing_method: ::vello::AaConfig::Msaa16"));
        // Area + Msaa8 markers must NOT appear in the AaConfig position.
        assert!(!rust.contains("antialiasing_method: ::vello::AaConfig::Area"));
        assert!(!rust.contains("antialiasing_method: ::vello::AaConfig::Msaa8"));
    }

    #[test]
    fn emits_renderer_vello_aa_msaa8_substitutes_struct_literal() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello" aa="msaa8"/>"#;
        let rust = compile_str(xml, "x.pinion.xml").expect("compile");
        assert!(rust.contains(
            "antialiasing_support: ::vello::AaSupport { area: false, msaa8: true, msaa16: false }"
        ));
        assert!(rust.contains("antialiasing_method: ::vello::AaConfig::Msaa8"));
    }

    #[test]
    fn emits_renderer_vello_aa_area_default_is_identical_to_explicit() {
        // The default-aa path (no attribute) and explicit aa="area"
        // must emit byte-identical Rust source — important for
        // build-cache reproducibility and for the AST equality check
        // in [`defaults_renderer_aa_to_area_when_absent`].
        let xml_default = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello"/>"#;
        let xml_explicit = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="vello" aa="area"/>"#;
        let rust_default = compile_str(xml_default, "x.pinion.xml").expect("compile default");
        let rust_explicit = compile_str(xml_explicit, "x.pinion.xml").expect("compile explicit");
        assert_eq!(rust_default, rust_explicit);
    }

    #[test]
    fn emits_renderer_vello_module_header_and_no_text_pollution() {
        // The emitted file opens with a generated-source banner so a
        // future reader looking at the OUT_DIR product knows not to
        // hand-edit. R46.3: regular `//` line comments, not `//!` —
        // the file is `include!()`d inside a `mod { ... }` wrap by
        // consumers and `//!` would be rejected as a misplaced inner
        // doc preceding the codegen's `use vello::...` items.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="Y" backend="vello"/>"#;
        let rust = compile_str(xml, "y.pinion.xml").expect("compile");
        assert!(rust.starts_with("// Generated by pinion-forge"));
        assert!(
            !rust.starts_with("//!"),
            "header must be regular `//`, not inner-doc `//!`"
        );
        assert!(rust.contains("DO NOT EDIT"));
        assert!(rust.contains("R46.2 §5.16 Vello first emit template"));
        // No leftover placeholders — the .replace() chain in
        // emit_renderer_vello must substitute all four (R46.2 +
        // R46.2.1).
        assert!(!rust.contains("__NAME__"));
        assert!(!rust.contains("__ERR_NAME__"));
        assert!(!rust.contains("__AA_SUPPORT__"));
        assert!(!rust.contains("__AA_METHOD__"));
        assert!(rust.ends_with('\n'));
    }

    #[test]
    fn renderer_wire_diagnostic_carries_backend_actual() {
        // UnknownBackend should surface the offending literal in the
        // wire `actual` field so an agent can repair it without
        // re-parsing the message text.
        let xml =
            r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="renderer" name="X" backend="ash"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unknown-backend")
            .expect("unknown-backend");
        let value = to_json_value(bad);
        assert_eq!(value["actual"], Value::from("ash"));
    }

    #[test]
    fn unknown_kind_message_mentions_renderer() {
        // After R46, the dsl/unknown-kind message must enumerate
        // renderer as a supported value alongside reactive.
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="view-fn" name="X"/>"#;
        let diags = parse_err(xml);
        let bad = diags
            .iter()
            .find(|d| d.code() == "dsl/unknown-kind")
            .expect("unknown-kind");
        let msg = bad.to_string();
        assert!(
            msg.contains("reactive"),
            "message should still list reactive"
        );
        assert!(msg.contains("renderer"), "message should list renderer");
    }
}
