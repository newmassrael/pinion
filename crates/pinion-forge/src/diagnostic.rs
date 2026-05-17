//! Closed diagnostic enum for the pinion DSL parser/codegen pipeline.
//!
//! Per R38 §5.22, pinion-forge owns its own diagnostic namespace
//! (`pinion::dsl::*` in Rust; `dsl/<kebab>` on the wire). The shape mirrors
//! SCE v1 NDJSON (`schemas/sce-diagnostic.v1.schema.json`) as a *reference
//! pattern* per RFC 001 closed policy — pinion does not extend SCE's
//! closed `DiagnosticCode` enum.
//!
//! Wire serialization lives in [`crate::wire`]; this module defines the
//! domain type and its (code, stage) classification only.

use std::path::PathBuf;

/// Source location for a diagnostic. Mirrors the SCE v1 `location` object:
/// `file` is required when the object is present; `line`/`column` are
/// optional.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub file: PathBuf,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

impl Location {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into(), line: None, column: None }
    }

    #[must_use]
    pub fn with_line_col(mut self, line: u32, column: u32) -> Self {
        self.line = Some(line);
        self.column = Some(column);
        self
    }
}

/// Pipeline stage. SCE v1 `stage` field analogue; closed by design,
/// extended by adding new variants in lockstep with [`PinionForgeDiagnostic`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// XML lexer/parser failed before any DSL-level validation.
    Parse,
    /// DSL-level validation: root tag, required attributes, identifier shape,
    /// unsupported child elements.
    Validate,
}

impl Stage {
    #[must_use]
    pub fn as_wire(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Validate => "validate",
        }
    }
}

/// Closed enum of pinion DSL diagnostics. New variants are append-only;
/// renaming or removing a variant requires a wire schema bump (no such
/// bump has happened yet — wire is `v=1`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PinionForgeDiagnostic {
    /// XML lexer/parser rejected the input. Source location is best-effort
    /// from quick-xml's buffer position.
    #[error("XML parse error in {}: {message}", location.file.display())]
    XmlParseError { message: String, location: Location },

    /// Root element was not `<pinion>`. `found` is the literal local name.
    #[error("invalid root <{found}> in {}: expected <pinion>", location.file.display())]
    InvalidRoot { found: String, location: Location },

    /// `<pinion>` root is missing the required `xmlns` attribute (the
    /// pinion DSL namespace claim — see [`PINION_DSL_NS`]).
    #[error("<pinion> in {} missing required xmlns attribute (expected {expected})", location.file.display())]
    MissingXmlns { expected: &'static str, location: Location },

    /// `<pinion xmlns=...>` declared a namespace other than the canonical
    /// pinion DSL namespace.
    #[error("<pinion xmlns=\"{found}\"> in {}: expected {expected}", location.file.display())]
    WrongXmlns { found: String, expected: &'static str, location: Location },

    /// `<pinion>` root is missing the required `kind` attribute.
    #[error("<pinion> in {} missing required kind attribute", location.file.display())]
    MissingKind { location: Location },

    /// `kind="..."` value is not in the supported set. R38.1 introduced
    /// `"reactive"`; R46 §5.16 added `"renderer"`. Future kinds attach
    /// here as additional accepted literals (spec round per addition).
    #[error("<pinion kind=\"{found}\"> in {}: only \"reactive\" / \"renderer\" supported", location.file.display())]
    UnknownKind { found: String, location: Location },

    /// `<pinion>` root is missing the required `name` attribute.
    #[error("<pinion> in {} missing required name attribute", location.file.display())]
    MissingName { location: Location },

    /// `name="..."` is not a valid Rust identifier (would corrupt codegen).
    #[error("<pinion name=\"{found}\"> in {}: name must be a valid Rust identifier", location.file.display())]
    InvalidName { found: String, location: Location },

    /// Child element of `<pinion>` is not in the supported set. R38.2a
    /// supports `<signal>`; `<computed>` / `<resource>` / `<use>` land
    /// in subsequent slices.
    #[error("<{tag}> inside <pinion> in {}: unsupported element", location.file.display())]
    UnsupportedElement { tag: String, location: Location },

    /// Generic missing-attribute diagnostic for child elements (e.g.
    /// `<signal name=...>` is required). The closed root attribute set
    /// has dedicated variants ([`Self::MissingXmlns`] / [`Self::MissingKind`] /
    /// [`Self::MissingName`]) because authoring guidance is different
    /// there.
    #[error("<{tag}> in {} missing required {attribute} attribute", location.file.display())]
    MissingAttribute { tag: String, attribute: String, location: Location },

    /// Generic invalid-identifier diagnostic for child-element attribute
    /// values that must be valid Rust identifiers (`<signal name=...>`,
    /// `<computed name=...>`, etc.). [`Self::InvalidName`] handles the
    /// root-element `<pinion name=...>` case.
    #[error("<{tag} {attribute}=\"{found}\"> in {}: must be a valid Rust identifier", location.file.display())]
    InvalidIdent { tag: String, attribute: String, found: String, location: Location },

    /// Required body content (text/CDATA) is missing or whitespace-only.
    /// Used by elements that mandate an inline expression — e.g.
    /// `<signal>` carries the initial-value expression in its body.
    #[error("<{tag}> in {} missing required body content", location.file.display())]
    EmptyBody { tag: String, location: Location },

    /// `<pinion kind="renderer">` is missing the required `backend`
    /// attribute. R46 §5.16: the renderer kind selects its codegen
    /// template by `backend` (no default — the choice is load-bearing
    /// for build-time per-target selection per R11 zero-overhead).
    #[error("<pinion kind=\"renderer\"> in {} missing required backend attribute", location.file.display())]
    MissingBackend { location: Location },

    /// `<pinion kind="renderer" backend="...">` value is not in the
    /// supported set. R46 commit 1 introduces only `"vello"`; R41 Phase
    /// 2/3/4 adds thin-RHI / custom-pass / B3 as additional accepted
    /// literals (spec round per addition).
    #[error("<pinion kind=\"renderer\" backend=\"{found}\"> in {}: only \"vello\" supported at R46", location.file.display())]
    UnknownBackend { found: String, location: Location },

    /// `<pinion kind="renderer">` carries a child element. The renderer
    /// kind is self-closing — its payload is entirely in root
    /// attributes — so any child is a schema violation. R46 §5.16.
    #[error("<{tag}> inside <pinion kind=\"renderer\"> in {}: renderer kind takes no children", location.file.display())]
    RendererChildNotAllowed { tag: String, location: Location },
}

/// Canonical pinion DSL namespace URI. `<pinion xmlns="...">` must match
/// this string exactly. Bumping the trailing `/v1` is reserved for a
/// schema break; additive `<pinion>` attributes/children do *not* warrant
/// a namespace bump.
pub const PINION_DSL_NS: &str = "https://pinion.dev/dsl/v1";

impl PinionForgeDiagnostic {
    /// Slash-form wire code. Matches SCE v1 conventions (kebab-case after
    /// `<category>/`). The `dsl/` prefix scopes diagnostics to the pinion
    /// DSL parser/codegen; future stages (e.g. `runtime/...`) get their
    /// own prefix.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::XmlParseError { .. } => "dsl/xml-parse",
            Self::InvalidRoot { .. } => "dsl/invalid-root",
            Self::MissingXmlns { .. } => "dsl/missing-xmlns",
            Self::WrongXmlns { .. } => "dsl/wrong-xmlns",
            Self::MissingKind { .. } => "dsl/missing-kind",
            Self::UnknownKind { .. } => "dsl/unknown-kind",
            Self::MissingName { .. } => "dsl/missing-name",
            Self::InvalidName { .. } => "dsl/invalid-name",
            Self::UnsupportedElement { .. } => "dsl/unsupported-element",
            Self::MissingAttribute { .. } => "dsl/missing-attribute",
            Self::InvalidIdent { .. } => "dsl/invalid-ident",
            Self::EmptyBody { .. } => "dsl/empty-body",
            Self::MissingBackend { .. } => "dsl/missing-backend",
            Self::UnknownBackend { .. } => "dsl/unknown-backend",
            Self::RendererChildNotAllowed { .. } => "dsl/renderer-child-not-allowed",
        }
    }

    /// Pipeline stage the diagnostic was raised at. Drives downstream
    /// dispatch (which repair loop owns this failure mode).
    #[must_use]
    pub fn stage(&self) -> Stage {
        match self {
            Self::XmlParseError { .. } => Stage::Parse,
            Self::InvalidRoot { .. }
            | Self::MissingXmlns { .. }
            | Self::WrongXmlns { .. }
            | Self::MissingKind { .. }
            | Self::UnknownKind { .. }
            | Self::MissingName { .. }
            | Self::InvalidName { .. }
            | Self::UnsupportedElement { .. }
            | Self::MissingAttribute { .. }
            | Self::InvalidIdent { .. }
            | Self::EmptyBody { .. }
            | Self::MissingBackend { .. }
            | Self::UnknownBackend { .. }
            | Self::RendererChildNotAllowed { .. } => Stage::Validate,
        }
    }

    /// Source location accessor. Every variant carries a [`Location`] (the
    /// parser always knows the file path even when row/col are unknown).
    #[must_use]
    pub fn location(&self) -> &Location {
        match self {
            Self::XmlParseError { location, .. }
            | Self::InvalidRoot { location, .. }
            | Self::MissingXmlns { location, .. }
            | Self::WrongXmlns { location, .. }
            | Self::MissingKind { location, .. }
            | Self::UnknownKind { location, .. }
            | Self::MissingName { location, .. }
            | Self::InvalidName { location, .. }
            | Self::UnsupportedElement { location, .. }
            | Self::MissingAttribute { location, .. }
            | Self::InvalidIdent { location, .. }
            | Self::EmptyBody { location, .. }
            | Self::MissingBackend { location, .. }
            | Self::UnknownBackend { location, .. }
            | Self::RendererChildNotAllowed { location, .. } => location,
        }
    }
}
