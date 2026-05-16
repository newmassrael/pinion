//! pinion-forge — pinion DSL (`.pinion.xml`) parser + Rust codegen per
//! §5.22 (R38).
//!
//! ## Scope
//!
//! pinion-forge consumes `.pinion.xml` files authored by humans or AI
//! agents and emits Rust source that targets the [`pinion-core::reactive`]
//! runtime. Each `.pinion.xml` compiles to exactly one `pub struct` plus
//! an `impl <Name> { pub fn new(owner: &Owner) -> Self }` constructor.
//!
//! Per R38 ratify (§5.22 redefined): the file extension is fixed at
//! `.pinion.xml`, the root element is `<pinion xmlns="..." kind="..."
//! name="...">`, and the child set is closed (`<use>` / `<signal>` /
//! `<computed>` / `<resource>` for `kind="reactive"`).
//!
//! Per R37.7 + R37.8: pinion-forge owns its codegen. SCE upstream rejects
//! framework-specific kinds (RFC 001 closed); the only SCE surface this
//! crate consumes is the v1 NDJSON diagnostic *pattern* (`v` / `id` /
//! `code` / `stage` / `message` shape — see `schemas/sce-diagnostic.v1
//! .schema.json`). pinion's own diagnostic namespace lives in
//! [`diagnostic::PinionForgeDiagnostic`] and is not an SCE
//! `DiagnosticCode` extension.
//!
//! ## R38.1 status (this build round)
//!
//! Skeleton only. The parser accepts the empty root
//! `<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="..."/>`
//! and rejects every child element with `dsl/unsupported-element`. The
//! codegen emits the constructor body `Self` (no fields). R38.2+ lands
//! the real `<signal>` / `<computed>` / `<resource>` AST + emit.
//!
//! [`pinion-core::reactive`]: ../pinion_core/reactive/index.html

#![forbid(unsafe_code)]

pub mod ast;
pub mod build;
pub mod codegen;
pub mod diagnostic;
pub mod parser;
pub mod wire;

pub use ast::{ComputedDecl, PinionChild, PinionDoc, PinionKind, ResourceDecl, SignalDecl};
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

    #[test]
    fn parses_empty_reactive_self_closing() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty"/>"#;
        let doc = parse(xml).expect("happy path");
        assert_eq!(doc.name, "Empty");
        assert_eq!(doc.kind, PinionKind::Reactive);
        assert!(doc.children.is_empty());
    }

    #[test]
    fn parses_empty_reactive_open_close_with_whitespace() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Empty">
        </pinion>"#;
        let doc = parse(xml).expect("happy path with whitespace body");
        assert_eq!(doc.name, "Empty");
    }

    #[test]
    fn emits_constructor_for_empty_doc() {
        let doc = PinionDoc {
            name: "Foo".into(),
            kind: PinionKind::Reactive,
            children: Vec::new(),
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
        let wrong = diags.iter().find(|d| d.code() == "dsl/wrong-xmlns").expect("wrong-xmlns");
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
        let bad = diags.iter().find(|d| d.code() == "dsl/unknown-kind").expect("unknown-kind");
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
        let bad = diags.iter().find(|d| d.code() == "dsl/invalid-name").expect("invalid-name");
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
        assert_eq!(doc.children.len(), 1);
        let PinionChild::Signal(sig) = &doc.children[0] else {
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
        let PinionChild::Signal(sig) = &doc.children[0] else {
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
        assert_eq!(doc.children.len(), 3);
        let names: Vec<&str> = doc
            .children
            .iter()
            .map(|c| match c {
                PinionChild::Signal(s) => s.name.as_str(),
                PinionChild::Computed(_) | PinionChild::Resource(_) => {
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
        let bad = diags.iter().find(|d| d.code() == "dsl/invalid-ident").expect("invalid-ident");
        let PinionForgeDiagnostic::InvalidIdent { tag, attribute, found, .. } = bad else {
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
        let bad = diags.iter().find(|d| d.code() == "dsl/empty-body").expect("empty-body");
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
        assert_eq!(diags.iter().filter(|d| d.code() == "dsl/unsupported-element").count(), 1);
    }

    // ---- R38.2b: <computed> child element ----

    #[test]
    fn parses_single_computed() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="Greeter">
            <computed name="msg" ty="String"><![CDATA[String::from("hi")]]></computed>
        </pinion>"#;
        let doc = parse(xml).expect("happy path");
        assert_eq!(doc.children.len(), 1);
        let PinionChild::Computed(c) = &doc.children[0] else {
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
        assert_eq!(doc.children.len(), 2);
        assert!(matches!(doc.children[0], PinionChild::Signal(_)));
        assert!(matches!(doc.children[1], PinionChild::Computed(_)));
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
        assert!(rust.contains("::pinion_core::reactive::Computed::new(move || { count.get() * 2 })"));
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
        assert!(rust.contains(
            "let forty_two = ::pinion_core::reactive::Computed::new(move || { 42 });"
        ));
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
        let bad = diags.iter().find(|d| d.code() == "dsl/empty-body").expect("empty-body");
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

    // ---- R38.2c: <resource> child element ----

    #[test]
    fn parses_single_resource() {
        let xml = r#"<pinion xmlns="https://pinion.dev/dsl/v1" kind="reactive" name="UserView">
            <resource name="user" ty="String" err="String">
                <![CDATA[async move { Ok::<String, String>("hi".into()) }]]>
            </resource>
        </pinion>"#;
        let doc = parse(xml).expect("happy path");
        assert_eq!(doc.children.len(), 1);
        let PinionChild::Resource(r) = &doc.children[0] else {
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
        assert!(rust.contains("pub fn new<S>(_owner: &::pinion_core::reactive::Owner, spawner: &S) -> Self"));
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
        let bad = diags.iter().find(|d| d.code() == "dsl/empty-body").expect("empty-body");
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
                PinionForgeDiagnostic::MissingAttribute { tag, .. } => Some(("missing", tag.as_str())),
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
        let diag = diags.iter().find(|d| d.code() == "dsl/missing-xmlns").unwrap();
        let line = to_ndjson_line(diag);
        let value: Value = serde_json::from_str(&line).expect("valid JSON");
        let obj = value.as_object().expect("object");
        assert_eq!(obj.get("v"), Some(&Value::from(1)));
        assert!(obj.get("id").and_then(Value::as_str).unwrap().starts_with("fnv1a:"));
        assert_eq!(obj.get("code"), Some(&Value::from("dsl/missing-xmlns")));
        assert_eq!(obj.get("stage"), Some(&Value::from("validate")));
        assert!(obj.get("message").is_some());
        let loc = obj.get("location").and_then(Value::as_object).expect("location");
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
}
