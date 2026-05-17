//! `.pinion.xml` → [`PinionDoc`] parser. Single-pass over the event stream
//! produced by `quick-xml`; collects every diagnostic encountered rather
//! than fail-fast so downstream tooling can show all errors at once
//! (textbook contract for AOT compilers — surface every failure per run).
//!
//! ## R38.2d grammar (closed)
//!
//! ```text
//! Document   ::= XmlDecl? Whitespace* PinionRoot Whitespace*
//! PinionRoot ::= '<pinion' RootAttr+ '/>'
//!             |  '<pinion' RootAttr+ '>' Whitespace* Child* '</pinion>'
//! RootAttr   ::= xmlns="https://pinion.dev/dsl/v1"
//!             |  kind="reactive"
//!             |  name=<Rust ident>
//! Child      ::= Signal | Computed | Resource | Use
//! Signal     ::= '<signal' NamedTypedAttrs '>' BodyExpr '</signal>'
//! Computed   ::= '<computed' NamedTypedAttrs '>' BodyExpr '</computed>'
//! Resource   ::= '<resource' NamedTypedAttrs 'err'=<non-empty> '>'
//!                BodyExpr '</resource>'
//! Use        ::= '<use' 'path'=<non-empty> '/>'
//!             |  '<use' 'path'=<non-empty> '>' (anything) '</use>'
//! NamedTypedAttrs ::= 'name'=<Rust ident> 'ty'=<non-empty>
//! BodyExpr   ::= CDATA / non-empty trimmed Text
//! ```
//!
//! `<use>` is the only child that ignores its body — the path
//! attribute is the entire payload. Any body content
//! (text/CDATA/nested elements) is silently skipped so an author
//! mistakenly closing the tag with `</use>` does not get spurious
//! diagnostics. Validation strictness is a [carry-forward R38.2x]
//! decision (syn-based path validation tied to broader syn adoption).

use std::path::PathBuf;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

use crate::ast::{
    ComputedDecl, PinionChild, PinionDoc, PinionKind, PinionSpec, RendererBackend,
    RendererBackendKind, ResourceDecl, SignalDecl, UseDecl, VelloAaMode,
};
use crate::diagnostic::{Location, PINION_DSL_NS, PinionForgeDiagnostic};

/// Entry point. `source` is the originating file path for diagnostic
/// labeling; `xml` is the contents. The file does not need to exist on
/// disk — the path is metadata only, never opened by the parser.
///
/// # Errors
/// Returns a non-empty `Vec` of [`PinionForgeDiagnostic`] on any
/// `Parse`- or `Validate`-stage failure (malformed XML, wrong root,
/// missing/invalid attributes, unsupported child element). All
/// diagnostics encountered in a single pass are surfaced together so
/// downstream tooling can show every failure per run.
pub fn parse_pinion(
    xml: &str,
    source: impl Into<PathBuf>,
) -> Result<PinionDoc, Vec<PinionForgeDiagnostic>> {
    let source = source.into();
    let mut ctx = ParseCtx::new(xml, source);
    ctx.parse_document()
}

/// Internal parser state. Holds the reader, the byte→(line,col) index
/// for diagnostic location synthesis, and the accumulating diagnostic
/// vector.
struct ParseCtx<'a> {
    reader: Reader<&'a [u8]>,
    source_bytes: &'a [u8],
    source_file: PathBuf,
    diagnostics: Vec<PinionForgeDiagnostic>,
}

impl<'a> ParseCtx<'a> {
    fn new(xml: &'a str, source_file: PathBuf) -> Self {
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);
        Self { reader, source_bytes: xml.as_bytes(), source_file, diagnostics: Vec::new() }
    }

    fn parse_document(&mut self) -> Result<PinionDoc, Vec<PinionForgeDiagnostic>> {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Eof) => {
                    if self.diagnostics.is_empty() {
                        // Reached EOF before finding any element — empty doc.
                        self.diagnostics.push(PinionForgeDiagnostic::InvalidRoot {
                            found: String::new(),
                            location: Location::new(self.source_file.clone()),
                        });
                    }
                    return Err(std::mem::take(&mut self.diagnostics));
                }
                Ok(Event::Start(e)) => return self.parse_root_open(&e, /*self_closing=*/ false),
                Ok(Event::Empty(e)) => return self.parse_root_open(&e, /*self_closing=*/ true),
                Ok(Event::Decl(_) | Event::DocType(_) | Event::Comment(_) | Event::PI(_)) => {}
                Ok(Event::Text(t)) => {
                    // Only whitespace is legal before the root element.
                    if !t.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r' | b'\n')) {
                        let location = self.current_location();
                        self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                            message: "unexpected text content before root element".into(),
                            location,
                        });
                    }
                }
                Ok(Event::End(_) | Event::CData(_)) => {
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: "unexpected event before root element".into(),
                        location,
                    });
                    return Err(std::mem::take(&mut self.diagnostics));
                }
                Err(e) => {
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: e.to_string(),
                        location,
                    });
                    return Err(std::mem::take(&mut self.diagnostics));
                }
            }
            buf.clear();
        }
    }

    /// Validate the `<pinion>` open tag. If the tag name itself is wrong,
    /// stop after emitting `InvalidRoot` — the rest of the document is
    /// not meaningful. If only attributes are wrong, accumulate every
    /// per-attribute diagnostic, then continue scanning for child-level
    /// diagnostics so a single run surfaces them all.
    ///
    /// Kind dispatch: after the core attribute checks, the kind selects
    /// which body scanner runs and which kind-specific attribute rule
    /// applies. `reactive` validates / scans children per R38.2a-d;
    /// `renderer` validates the `backend` attribute and rejects any
    /// child element (R46 §5.16). When `attrs_ok` is `None` (one of
    /// xmlns / kind / name failed), the scanner falls back to the
    /// reactive child grammar so a single run still surfaces every
    /// child-level problem alongside the attribute failure.
    fn parse_root_open(
        &mut self,
        start: &BytesStart<'_>,
        self_closing: bool,
    ) -> Result<PinionDoc, Vec<PinionForgeDiagnostic>> {
        let local = local_name(start.name());
        if local != "pinion" {
            let location = self.current_location();
            self.diagnostics.push(PinionForgeDiagnostic::InvalidRoot { found: local, location });
            return Err(std::mem::take(&mut self.diagnostics));
        }

        // Capture the root-tag location once for any kind-specific
        // diagnostics raised below (e.g. MissingBackend points to the
        // root, not to the first child).
        let location = self.current_location();
        let attrs_ok = self.parse_root_attrs(start);

        // Kind-specific attribute validation runs after the core attrs
        // pass — only renderer has additional load-bearing attributes
        // (backend + aa at R46.2.1). Reactive's only kind-specific
        // surface is the child element set, validated during body scan.
        let (backend_kind, aa) = match attrs_ok
            .as_ref()
            .map(|(k, b, a, _)| (*k, b.clone(), a.clone()))
        {
            Some((PinionKind::Renderer, backend_raw, aa_raw)) => (
                self.validate_renderer_backend(backend_raw, &location),
                self.validate_renderer_aa(aa_raw, &location),
            ),
            _ => (None, None),
        };

        let mut children: Vec<PinionChild> = Vec::new();
        if !self_closing {
            match attrs_ok.as_ref().map(|(k, _, _, _)| *k) {
                Some(PinionKind::Renderer) => self.scan_renderer_body(),
                // Reactive or unknown (kind failed) — fall back to the
                // reactive child grammar so we surface every child-level
                // issue alongside the attribute failure in a single run.
                _ => self.scan_root_body(&mut children),
            }
        }

        if self.diagnostics.is_empty() {
            // Invariant: parse_root_attrs returns Some iff no attribute
            // diagnostics were pushed; validate_renderer_* returns Some
            // iff no kind-specific diagnostic was pushed; scan_*_body
            // pushes diagnostics on failure. So an empty diagnostic vec
            // implies attrs_ok and every per-kind validator succeeded.
            let (kind, _, _, name) =
                attrs_ok.expect("attrs-ok path must populate (kind, backend, aa, name)");
            let spec = match kind {
                PinionKind::Reactive => PinionSpec::Reactive { children },
                PinionKind::Renderer => {
                    let backend_kind = backend_kind.expect(
                        "renderer-ok path must populate backend_kind (empty diagnostics implies validated)",
                    );
                    let aa = aa.expect(
                        "renderer-ok path must populate aa (validate_renderer_aa defaults to Area)",
                    );
                    PinionSpec::Renderer { backend: assemble_backend(backend_kind, aa) }
                }
            };
            Ok(PinionDoc { name, spec })
        } else {
            Err(std::mem::take(&mut self.diagnostics))
        }
    }

    /// Validate the `backend` attribute for `kind="renderer"`. Pushes
    /// [`PinionForgeDiagnostic::MissingBackend`] when the attribute is
    /// absent or whitespace-only, and [`PinionForgeDiagnostic::UnknownBackend`]
    /// when the literal is not in [`RendererBackendKind::from_attr`]'s
    /// accepted set. Returns the parsed [`RendererBackendKind`] on
    /// success; the kind-specific payload (e.g. `aa` for Vello) is
    /// assembled separately in [`Self::parse_root_open`].
    fn validate_renderer_backend(
        &mut self,
        backend_raw: Option<String>,
        location: &Location,
    ) -> Option<RendererBackendKind> {
        match backend_raw {
            None => {
                self.diagnostics.push(PinionForgeDiagnostic::MissingBackend {
                    location: location.clone(),
                });
                None
            }
            Some(literal) => {
                let trimmed = literal.trim();
                if trimmed.is_empty() {
                    // Whitespace-only treated as missing — matches the
                    // require_nonempty_attr policy for child elements.
                    self.diagnostics.push(PinionForgeDiagnostic::MissingBackend {
                        location: location.clone(),
                    });
                    None
                } else if let Some(b) = RendererBackendKind::from_attr(trimmed) {
                    Some(b)
                } else {
                    self.diagnostics.push(PinionForgeDiagnostic::UnknownBackend {
                        found: literal,
                        location: location.clone(),
                    });
                    None
                }
            }
        }
    }

    /// Validate the `aa` attribute for `kind="renderer"`. R46.2.1 §5.16
    /// — the attribute is *optional* (absent = default [`VelloAaMode::Area`],
    /// the UI canonical), so a `None`/whitespace-only input is not a
    /// diagnostic. Only an unrecognized literal raises
    /// [`PinionForgeDiagnostic::UnknownAa`]; returns `None` in that case
    /// so the caller skips spec assembly. Backward-compat with R46.1
    /// manifests (no `aa` attribute) is preserved by the default path.
    fn validate_renderer_aa(
        &mut self,
        aa_raw: Option<String>,
        location: &Location,
    ) -> Option<VelloAaMode> {
        match aa_raw {
            None => Some(VelloAaMode::Area),
            Some(literal) => {
                let trimmed = literal.trim();
                if trimmed.is_empty() {
                    Some(VelloAaMode::Area)
                } else if let Some(a) = VelloAaMode::from_attr(trimmed) {
                    Some(a)
                } else {
                    self.diagnostics.push(PinionForgeDiagnostic::UnknownAa {
                        found: literal,
                        location: location.clone(),
                    });
                    None
                }
            }
        }
    }

    /// Scan the body of a `<pinion kind="renderer">` element. The
    /// renderer kind takes no children — every encountered child raises
    /// [`PinionForgeDiagnostic::RendererChildNotAllowed`] and the
    /// subtree is skipped so the outer scan stays aligned with the
    /// close tag. Comments / whitespace / processing instructions pass
    /// through silently (the renderer kind has no body content to
    /// collect either).
    fn scan_renderer_body(&mut self) {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::End(_)) => return,
                Ok(Event::Start(e)) => {
                    let tag = local_name(e.name());
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::RendererChildNotAllowed {
                        tag,
                        location,
                    });
                    self.skip_subtree();
                }
                Ok(Event::Empty(e)) => {
                    let tag = local_name(e.name());
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::RendererChildNotAllowed {
                        tag,
                        location,
                    });
                }
                Ok(Event::Eof) => {
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: "unexpected EOF inside <pinion>".into(),
                        location,
                    });
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: e.to_string(),
                        location,
                    });
                    return;
                }
            }
            buf.clear();
        }
    }

    /// Extract and validate the three core required root attributes
    /// (xmlns / kind / name) and collect the kind-specific raw `backend`
    /// literal for downstream validation. Returns
    /// `Some((kind, backend_raw, name))` only when xmlns / kind / name
    /// all pass; `backend_raw` is the unvalidated author literal (or
    /// `None` when the attribute is absent) and is interpreted by the
    /// caller in [`Self::build_spec`] per the kind. Even one core
    /// failure makes the doc unrenderable, so we don't construct a
    /// partial AST.
    fn parse_root_attrs(
        &mut self,
        start: &BytesStart<'_>,
    ) -> Option<(PinionKind, Option<String>, Option<String>, String)> {
        let location = self.current_location();
        let mut xmlns: Option<String> = None;
        let mut kind_raw: Option<String> = None;
        let mut name_raw: Option<String> = None;
        let mut backend_raw: Option<String> = None;
        let mut aa_raw: Option<String> = None;

        for attr in start.attributes().with_checks(false) {
            let Ok(attr) = attr else {
                self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                    message: "malformed attribute on <pinion>".into(),
                    location: location.clone(),
                });
                continue;
            };
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = String::from_utf8_lossy(&attr.value).into_owned();
            match key.as_str() {
                "xmlns" => xmlns = Some(value),
                "kind" => kind_raw = Some(value),
                "name" => name_raw = Some(value),
                "backend" => backend_raw = Some(value),
                "aa" => aa_raw = Some(value),
                _ => {
                    // Unknown attributes on <pinion> are tolerated at R38.1
                    // for forward compatibility — additive attributes added
                    // in a future schema revision MUST not break older
                    // parsers. quick-xml gave us the chance to read them;
                    // we drop them silently per the SCE v1 wire policy
                    // ("consumers MUST ignore unknown fields"). `backend` /
                    // `aa` are known schema attributes (R46 §5.16, R46.2.1)
                    // — they are accepted on any kind here but only
                    // consumed by the renderer kind; reactive silently
                    // drops them under the same forward-compat policy.
                }
            }
        }

        let xmlns_ok = match &xmlns {
            None => {
                self.diagnostics.push(PinionForgeDiagnostic::MissingXmlns {
                    expected: PINION_DSL_NS,
                    location: location.clone(),
                });
                false
            }
            Some(ns) if ns == PINION_DSL_NS => true,
            Some(ns) => {
                self.diagnostics.push(PinionForgeDiagnostic::WrongXmlns {
                    found: ns.clone(),
                    expected: PINION_DSL_NS,
                    location: location.clone(),
                });
                false
            }
        };

        let kind = match kind_raw {
            None => {
                self.diagnostics
                    .push(PinionForgeDiagnostic::MissingKind { location: location.clone() });
                None
            }
            Some(literal) => {
                let parsed = PinionKind::from_attr(&literal);
                if parsed.is_none() {
                    self.diagnostics.push(PinionForgeDiagnostic::UnknownKind {
                        found: literal,
                        location: location.clone(),
                    });
                }
                parsed
            }
        };

        let name = match name_raw {
            None => {
                self.diagnostics
                    .push(PinionForgeDiagnostic::MissingName { location: location.clone() });
                None
            }
            Some(literal) => {
                if is_rust_ident(&literal) {
                    Some(literal)
                } else {
                    self.diagnostics.push(PinionForgeDiagnostic::InvalidName {
                        found: literal,
                        location: location.clone(),
                    });
                    None
                }
            }
        };

        match (xmlns_ok, kind, name) {
            (true, Some(k), Some(n)) => Some((k, backend_raw, aa_raw, n)),
            _ => None,
        }
    }

    /// Scan from after `<pinion ...>` open through to `</pinion>`,
    /// dispatching each child element to the appropriate handler.
    /// Unsupported elements raise `UnsupportedElement` and their subtree
    /// is skipped so the scan stays aligned with the close tag.
    fn scan_root_body(&mut self, children: &mut Vec<PinionChild>) {
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::End(_)) => return,
                Ok(Event::Start(e)) => self.dispatch_child(&e, /*self_closing=*/ false, children),
                Ok(Event::Empty(e)) => self.dispatch_child(&e, /*self_closing=*/ true, children),
                Ok(Event::Eof) => {
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: "unexpected EOF inside <pinion>".into(),
                        location,
                    });
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    let location = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: e.to_string(),
                        location,
                    });
                    return;
                }
            }
            buf.clear();
        }
    }

    /// Route a child element to its parser. The dispatch table is closed
    /// per R38.2a — `<signal>` is the only recognized child. Anything
    /// else surfaces an `UnsupportedElement` and is skipped subtree-deep
    /// so the outer scan resumes at the correct close tag.
    fn dispatch_child(
        &mut self,
        start: &BytesStart<'_>,
        self_closing: bool,
        children: &mut Vec<PinionChild>,
    ) {
        let tag = local_name(start.name());
        // Dispatch table on child element name. R38.2a/b/c recognize
        // `<signal>` / `<computed>` / `<resource>`; R38.2d adds `<use>`.
        match tag.as_str() {
            "signal" => {
                if let Some(decl) = self.parse_signal(start, self_closing) {
                    children.push(PinionChild::Signal(decl));
                }
            }
            "computed" => {
                if let Some(decl) = self.parse_computed(start, self_closing) {
                    children.push(PinionChild::Computed(decl));
                }
            }
            "resource" => {
                if let Some(decl) = self.parse_resource(start, self_closing) {
                    children.push(PinionChild::Resource(decl));
                }
            }
            "use" => {
                let decl = self.parse_use(start);
                // <use> ignores body content (path attribute is the whole
                // payload). Skip to the close tag regardless of whether
                // the AST decl was constructed.
                if !self_closing {
                    self.skip_subtree();
                }
                if let Some(decl) = decl {
                    children.push(PinionChild::Use(decl));
                }
            }
            _ => {
                let location = self.current_location();
                self.diagnostics.push(PinionForgeDiagnostic::UnsupportedElement {
                    tag,
                    location,
                });
                if !self_closing {
                    self.skip_subtree();
                }
            }
        }
    }

    /// Parse one `<signal name="..." ty="...">body</signal>` element.
    /// Returns `None` when any part is malformed — diagnostics
    /// accumulate in `self.diagnostics`.
    fn parse_signal(
        &mut self,
        start: &BytesStart<'_>,
        self_closing: bool,
    ) -> Option<SignalDecl> {
        self.parse_named_typed_body("signal", start, self_closing)
            .map(|(name, ty, initial)| SignalDecl { name, ty, initial })
    }

    /// Parse one `<computed name="..." ty="...">body</computed>` element.
    /// Surface validation is identical to `<signal>` — the divergence is
    /// what [`crate::codegen`] does with `body` (closure body vs initial
    /// value).
    fn parse_computed(
        &mut self,
        start: &BytesStart<'_>,
        self_closing: bool,
    ) -> Option<ComputedDecl> {
        self.parse_named_typed_body("computed", start, self_closing)
            .map(|(name, ty, body)| ComputedDecl { name, ty, body })
    }

    /// Parse one `<resource name="..." ty="..." err="...">body</resource>`
    /// element. Distinct from `<signal>` / `<computed>` because of the
    /// fourth required attribute (`err`) — the `(name, ty, err, body)`
    /// shape doesn't fit [`Self::parse_named_typed_body`] without
    /// over-generalizing the helper. A future builder-style refactor
    /// (R38.2x) may collapse all three element parsers.
    fn parse_resource(
        &mut self,
        start: &BytesStart<'_>,
        self_closing: bool,
    ) -> Option<ResourceDecl> {
        let location = self.current_location();
        let mut name_raw: Option<String> = None;
        let mut ty_raw: Option<String> = None;
        let mut err_raw: Option<String> = None;

        for attr in start.attributes().with_checks(false) {
            let Ok(attr) = attr else {
                self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                    message: "malformed attribute on <resource>".into(),
                    location: location.clone(),
                });
                continue;
            };
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = String::from_utf8_lossy(&attr.value).into_owned();
            match key.as_str() {
                "name" => name_raw = Some(value),
                "ty" => ty_raw = Some(value),
                "err" => err_raw = Some(value),
                _ => {
                    // Unknown attributes tolerated per SCE v1 forward-
                    // compat policy.
                }
            }
        }

        let name = self.require_ident_attr("resource", "name", name_raw, &location);
        let ty = self.require_nonempty_attr("resource", "ty", ty_raw, &location);
        let err = self.require_nonempty_attr("resource", "err", err_raw, &location);
        let body = if self_closing {
            self.diagnostics.push(PinionForgeDiagnostic::EmptyBody {
                tag: "resource".into(),
                location,
            });
            None
        } else {
            self.scan_text_body("resource")
        };

        match (name, ty, err, body) {
            (Some(name), Some(ty), Some(err), Some(body)) => {
                Some(ResourceDecl { name, ty, err, body })
            }
            _ => None,
        }
    }

    /// Parse one `<use path="..."/>` element. Body content (if any) is
    /// not consumed here — the dispatcher calls [`Self::skip_subtree`]
    /// for the close-tag form. Returns `None` only if the required
    /// `path` attribute is missing or empty.
    fn parse_use(&mut self, start: &BytesStart<'_>) -> Option<UseDecl> {
        let location = self.current_location();
        let mut path_raw: Option<String> = None;

        for attr in start.attributes().with_checks(false) {
            let Ok(attr) = attr else {
                self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                    message: "malformed attribute on <use>".into(),
                    location: location.clone(),
                });
                continue;
            };
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = String::from_utf8_lossy(&attr.value).into_owned();
            if key == "path" {
                path_raw = Some(value);
            }
            // Unknown attributes tolerated (SCE v1 forward-compat).
        }

        // `path` is a free-form Rust use-path, not a Rust identifier —
        // can contain `::`, braces, `as`, `*`. R38.2d only enforces
        // non-emptiness; rustc owns the path syntax.
        let path = self.require_nonempty_attr("use", "path", path_raw, &location)?;
        Some(UseDecl { path })
    }

    /// Generic parser for the `(name, ty, body)` element shape shared by
    /// `<signal>` and `<computed>` (R38.2a/b) and reused by future
    /// child elements that carry the same surface contract. Returns
    /// `None` if any of the three fields is missing or invalid;
    /// individual diagnostics are pushed for each failure so a single
    /// run surfaces every problem.
    fn parse_named_typed_body(
        &mut self,
        tag: &'static str,
        start: &BytesStart<'_>,
        self_closing: bool,
    ) -> Option<(String, String, String)> {
        let location = self.current_location();
        let mut name_raw: Option<String> = None;
        let mut ty_raw: Option<String> = None;

        for attr in start.attributes().with_checks(false) {
            let Ok(attr) = attr else {
                self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                    message: format!("malformed attribute on <{tag}>"),
                    location: location.clone(),
                });
                continue;
            };
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let value = String::from_utf8_lossy(&attr.value).into_owned();
            match key.as_str() {
                "name" => name_raw = Some(value),
                "ty" => ty_raw = Some(value),
                _ => {
                    // Unknown attributes are tolerated per the SCE v1
                    // forward-compat policy. A future schema revision
                    // may add e.g. `eager="false"` to <signal> without
                    // breaking older parsers.
                }
            }
        }

        let name = self.require_ident_attr(tag, "name", name_raw, &location);
        let ty = self.require_nonempty_attr(tag, "ty", ty_raw, &location);
        let body = if self_closing {
            self.diagnostics
                .push(PinionForgeDiagnostic::EmptyBody { tag: tag.into(), location });
            None
        } else {
            self.scan_text_body(tag)
        };

        match (name, ty, body) {
            (Some(name), Some(ty), Some(body)) => Some((name, ty, body)),
            _ => None,
        }
    }

    /// Collect the body of a `<tag>...</tag>` element as a trimmed
    /// expression-or-statements string. Accepts plain `Text` and
    /// `CDATA`; nested elements raise `UnsupportedElement`. Whitespace-
    /// only bodies fail with `EmptyBody`. `tag` flows into diagnostics
    /// so the user sees `<signal>` vs `<computed>` in error messages.
    fn scan_text_body(&mut self, tag: &'static str) -> Option<String> {
        let location = self.current_location();
        let mut body = String::new();
        let mut buf = Vec::new();
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::End(_)) => {
                    let trimmed = body.trim().to_owned();
                    if trimmed.is_empty() {
                        self.diagnostics.push(PinionForgeDiagnostic::EmptyBody {
                            tag: tag.into(),
                            location,
                        });
                        return None;
                    }
                    return Some(trimmed);
                }
                Ok(Event::CData(c)) => {
                    body.push_str(&String::from_utf8_lossy(c.as_ref()));
                }
                Ok(Event::Text(t)) => {
                    body.push_str(&String::from_utf8_lossy(t.as_ref()));
                }
                Ok(Event::Start(e)) => {
                    let nested = local_name(e.name());
                    let loc_nested = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::UnsupportedElement {
                        tag: nested,
                        location: loc_nested,
                    });
                    self.skip_subtree();
                }
                Ok(Event::Empty(e)) => {
                    let nested = local_name(e.name());
                    let loc_nested = self.current_location();
                    self.diagnostics.push(PinionForgeDiagnostic::UnsupportedElement {
                        tag: nested,
                        location: loc_nested,
                    });
                }
                Ok(Event::Eof) => {
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: format!("unexpected EOF inside <{tag}>"),
                        location,
                    });
                    return None;
                }
                Ok(_) => {}
                Err(e) => {
                    self.diagnostics.push(PinionForgeDiagnostic::XmlParseError {
                        message: e.to_string(),
                        location,
                    });
                    return None;
                }
            }
            buf.clear();
        }
    }

    /// Skip events until the matching close tag for the element just
    /// opened. Used after an `UnsupportedElement` diagnostic to keep
    /// the outer scan aligned with the document structure.
    fn skip_subtree(&mut self) {
        let mut buf = Vec::new();
        let mut depth: usize = 1;
        while depth > 0 {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::Start(_)) => depth += 1,
                Ok(Event::End(_)) => depth -= 1,
                Ok(Event::Eof) | Err(_) => return,
                _ => {}
            }
            buf.clear();
        }
    }

    /// Validate that a required child-element attribute is present and a
    /// valid Rust identifier. Pushes the appropriate diagnostic on
    /// failure and returns `None`; on success returns the trimmed value.
    fn require_ident_attr(
        &mut self,
        tag: &str,
        attribute: &str,
        value: Option<String>,
        location: &Location,
    ) -> Option<String> {
        match value {
            None => {
                self.diagnostics.push(PinionForgeDiagnostic::MissingAttribute {
                    tag: tag.into(),
                    attribute: attribute.into(),
                    location: location.clone(),
                });
                None
            }
            Some(v) if is_rust_ident(&v) => Some(v),
            Some(v) => {
                self.diagnostics.push(PinionForgeDiagnostic::InvalidIdent {
                    tag: tag.into(),
                    attribute: attribute.into(),
                    found: v,
                    location: location.clone(),
                });
                None
            }
        }
    }

    /// Validate that a required child-element attribute is present and
    /// non-empty (after trimming). Less strict than [`Self::require_ident_attr`] —
    /// used for free-form attributes like `<signal ty="...">` where the
    /// value is a Rust type expression rather than an identifier.
    fn require_nonempty_attr(
        &mut self,
        tag: &str,
        attribute: &str,
        value: Option<String>,
        location: &Location,
    ) -> Option<String> {
        match value {
            None => {
                self.diagnostics.push(PinionForgeDiagnostic::MissingAttribute {
                    tag: tag.into(),
                    attribute: attribute.into(),
                    location: location.clone(),
                });
                None
            }
            Some(v) => {
                let trimmed = v.trim().to_owned();
                if trimmed.is_empty() {
                    self.diagnostics.push(PinionForgeDiagnostic::MissingAttribute {
                        tag: tag.into(),
                        attribute: attribute.into(),
                        location: location.clone(),
                    });
                    None
                } else {
                    Some(trimmed)
                }
            }
        }
    }

    fn current_location(&self) -> Location {
        let pos = self.reader.buffer_position();
        let (line, col) = byte_to_line_col(self.source_bytes, pos);
        Location::new(self.source_file.clone()).with_line_col(line, col)
    }
}

/// Combine a [`RendererBackendKind`] tag with the parsed kind-specific
/// payloads to produce the final [`RendererBackend`]. R46.2.1: only the
/// Vello arm exists, carrying the `aa` payload. Future backends
/// (Headless, Softbuffer, thin-RHI) will add arms that consume their
/// own kind-specific attributes — keeping the match exhaustive ensures
/// parser changes flag every assembly site at compile time.
fn assemble_backend(kind: RendererBackendKind, aa: VelloAaMode) -> RendererBackend {
    match kind {
        RendererBackendKind::Vello => RendererBackend::Vello { aa },
    }
}

/// Extract the local name of a qualified XML name as an owned `String`,
/// dropping any namespace prefix. quick-xml returns the name borrowed
/// from the reader's buffer — that buffer is reused on the next event,
/// so callers that retain the name (e.g. inside a diagnostic record)
/// must own the bytes.
fn local_name(name: QName<'_>) -> String {
    String::from_utf8_lossy(name.local_name().as_ref()).into_owned()
}

/// `byte_to_line_col` converts a byte offset into a 1-based (line, col)
/// pair. Linear scan — O(n) on the input. Acceptable for build-time
/// diagnostics; if `.pinion.xml` files ever grow to MB-scale we revisit
/// with a prefix index.
fn byte_to_line_col(src: &[u8], pos: u64) -> (u32, u32) {
    let cap = src.len().min(usize::try_from(pos).unwrap_or(usize::MAX));
    let mut line: u32 = 1;
    let mut col: u32 = 1;
    for &b in &src[..cap] {
        if b == b'\n' {
            line += 1;
            col = 1;
        } else {
            col += 1;
        }
    }
    (line, col)
}

/// Conservative Rust identifier check. Restricted to ASCII
/// `[A-Za-z_][A-Za-z0-9_]*` plus a small keyword blacklist; non-ASCII
/// identifiers are legal in Rust but rejected here to keep generated
/// symbols readable across tooling that doesn't normalize Unicode.
/// Bumping the policy is a deliberate R38.2+ decision.
fn is_rust_ident(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().expect("non-empty checked above");
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    !is_rust_keyword(s)
}

/// Reserved Rust keywords as of edition 2024. Sourced from the
/// Reference book ch. Keywords. Used by [`is_rust_ident`] only.
fn is_rust_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "crate"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "self"
            | "Self"
            | "static"
            | "struct"
            | "super"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "union"
            | "_"
            | "gen"
    )
}

