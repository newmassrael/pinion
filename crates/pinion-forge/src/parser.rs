//! `.pinion.xml` → [`PinionDoc`] parser. Single-pass over the event stream
//! produced by `quick-xml`; collects every diagnostic encountered rather
//! than fail-fast so downstream tooling can show all errors at once
//! (textbook contract for AOT compilers — surface every failure per run).
//!
//! ## R38.1 grammar (closed)
//!
//! ```text
//! Document   ::= XmlDecl? Whitespace* PinionRoot Whitespace*
//! PinionRoot ::= '<pinion' Attribute+ '/>'
//!             |  '<pinion' Attribute+ '>' Whitespace* '</pinion>'
//! Attribute  ::= xmlns="https://pinion.dev/dsl/v1"
//!             |  kind="reactive"
//!             |  name=<Rust ident>
//! ```
//!
//! Any deviation produces one or more [`PinionForgeDiagnostic`] records
//! and the parser returns `Err(Vec<_>)`. R38.2+ extends the child set;
//! the grammar above is what R38.1 ratifies.

use std::path::PathBuf;

use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;

use crate::ast::{PinionDoc, PinionKind};
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

        let doc_if_attrs_ok = self
            .parse_root_attrs(start)
            .map(|(kind, name)| PinionDoc { name, kind, children: Vec::new() });

        if !self_closing {
            self.scan_root_body();
        }

        if self.diagnostics.is_empty() {
            // SAFETY (invariant): parse_root_attrs returned Some iff no
            // attribute diagnostics were pushed; scan_root_body only
            // pushes diagnostics. So an empty diagnostic vec implies
            // doc_if_attrs_ok is Some.
            Ok(doc_if_attrs_ok.expect("attrs-ok path must populate doc"))
        } else {
            Err(std::mem::take(&mut self.diagnostics))
        }
    }

    /// Extract and validate the three required root attributes. Returns
    /// `Some((kind, name))` only if all three checks pass — even one
    /// failure makes the doc unrenderable, so we don't construct a
    /// partial AST.
    fn parse_root_attrs(&mut self, start: &BytesStart<'_>) -> Option<(PinionKind, String)> {
        let location = self.current_location();
        let mut xmlns: Option<String> = None;
        let mut kind_raw: Option<String> = None;
        let mut name_raw: Option<String> = None;

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
                _ => {
                    // Unknown attributes on <pinion> are tolerated at R38.1
                    // for forward compatibility — additive attributes added
                    // in a future schema revision MUST not break older
                    // parsers. quick-xml gave us the chance to read them;
                    // we drop them silently per the SCE v1 wire policy
                    // ("consumers MUST ignore unknown fields").
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
            (true, Some(k), Some(n)) => Some((k, n)),
            _ => None,
        }
    }

    /// Scan from after `<pinion ...>` open through to `</pinion>`. Every
    /// element child raises `UnsupportedElement` (R38.1: no children
    /// allowed). Whitespace text and comments are ignored.
    fn scan_root_body(&mut self) {
        let mut buf = Vec::new();
        let mut depth: usize = 0;
        loop {
            match self.reader.read_event_into(&mut buf) {
                Ok(Event::End(_)) if depth == 0 => return,
                Ok(Event::End(_)) => depth -= 1,
                Ok(Event::Start(e)) => {
                    let tag = local_name(e.name());
                    let location = self.current_location();
                    self.diagnostics
                        .push(PinionForgeDiagnostic::UnsupportedElement { tag, location });
                    depth += 1;
                }
                Ok(Event::Empty(e)) => {
                    let tag = local_name(e.name());
                    let location = self.current_location();
                    self.diagnostics
                        .push(PinionForgeDiagnostic::UnsupportedElement { tag, location });
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

    fn current_location(&self) -> Location {
        let pos = self.reader.buffer_position();
        let (line, col) = byte_to_line_col(self.source_bytes, pos);
        Location::new(self.source_file.clone()).with_line_col(line, col)
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

