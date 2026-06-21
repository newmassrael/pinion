//! `build.rs` helper API for downstream crates. The typical consumer is
//! a `build.rs` script:
//!
//! ```rust,ignore
//! // <consumer>/build.rs
//! fn main() {
//!     let out_dir = std::env::var_os("OUT_DIR").expect("cargo sets OUT_DIR");
//!     pinion_forge::build::compile_file(
//!         "src/ui/empty.pinion.xml",
//!         std::path::Path::new(&out_dir),
//!     )
//!     .expect("pinion-forge codegen");
//! }
//! ```
//!
//! Diagnostics are surfaced via the returned `Result` — callers decide
//! whether to print human form (`Display`) or NDJSON
//! ([`crate::wire::to_ndjson_line`]). `build.rs` typically logs human
//! form to stderr and panics on error so `cargo` shows the full message.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::codegen::emit_rust;
use crate::diagnostic::PinionForgeDiagnostic;
use crate::parser::parse_pinion;

/// Errors produced by [`compile_file`] and [`compile_str`]. Separated
/// from [`PinionForgeDiagnostic`] because I/O failures (`OUT_DIR` write
/// rejected, input file unreadable) are infrastructure failures, not
/// pinion-DSL syntax failures — they don't carry a (code, stage) the
/// agent can repair, and folding them into the DSL diagnostic enum
/// would corrupt the closed-set contract.
#[derive(Debug, thiserror::Error)]
pub enum CompileError {
    /// One or more pinion-DSL diagnostics. The vector is non-empty.
    #[error("pinion-forge: {} diagnostic(s)", .0.len())]
    Diagnostics(Vec<PinionForgeDiagnostic>),
    /// Underlying I/O failure (open / read / write / mkdir). The path
    /// is preserved for the message and is unrelated to any DSL source
    /// path that may appear in diagnostics.
    #[error("pinion-forge I/O error at {}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

impl From<Vec<PinionForgeDiagnostic>> for CompileError {
    fn from(v: Vec<PinionForgeDiagnostic>) -> Self {
        Self::Diagnostics(v)
    }
}

/// Compile an XML string to a Rust source string. `source_label` flows
/// into every diagnostic's `Location.file`; pass the originating path
/// even when the bytes came from memory so error reports stay anchored
/// to the real source.
///
/// # Errors
/// Returns [`CompileError::Diagnostics`] on any DSL-level failure
/// (parse, validate). Never returns I/O errors — pure in-memory.
pub fn compile_str(xml: &str, source_label: impl Into<PathBuf>) -> Result<String, CompileError> {
    let doc = parse_pinion(xml, source_label)?;
    Ok(emit_rust(&doc))
}

/// Read `input` from disk, compile, write the result to
/// `<output_dir>/<stem>.rs`. Returns the absolute path of the written
/// file so the caller can hand it to `include!` or report it.
///
/// `<stem>` is `<input>.file_stem()` with any `.pinion` suffix stripped
/// (e.g. `button.pinion.xml` → `button.rs`). This keeps generated names
/// matching the source-tree convention.
///
/// # Errors
/// - [`CompileError::Io`] if `input` is unreadable, or `output_dir` (or
///   the resulting `.rs` path) cannot be created/written.
/// - [`CompileError::Diagnostics`] on any DSL-level failure.
pub fn compile_file(
    input: impl AsRef<Path>,
    output_dir: impl AsRef<Path>,
) -> Result<PathBuf, CompileError> {
    let input = input.as_ref();
    let output_dir = output_dir.as_ref();

    let xml = fs::read_to_string(input).map_err(|source| CompileError::Io {
        path: input.to_path_buf(),
        source,
    })?;

    // Use the input path as the diagnostic source label so any error
    // refers to the on-disk file the user authored.
    let rust = compile_str(&xml, input)?;

    fs::create_dir_all(output_dir).map_err(|source| CompileError::Io {
        path: output_dir.to_path_buf(),
        source,
    })?;

    let out_path = output_dir.join(derived_stem(input));
    fs::write(&out_path, rust).map_err(|source| CompileError::Io {
        path: out_path.clone(),
        source,
    })?;
    Ok(out_path)
}

/// Strip the conventional `.pinion.xml` double-extension. Falls back to
/// the `file_stem()` for any other shape, then appends `.rs`.
fn derived_stem(input: &Path) -> PathBuf {
    let file_name = input
        .file_name()
        .map_or_else(|| "pinion".into(), |s| s.to_string_lossy().into_owned());
    let stem = file_name.strip_suffix(".pinion.xml").map_or_else(
        || {
            Path::new(&file_name)
                .file_stem()
                .map_or_else(|| "pinion".into(), |s| s.to_string_lossy().into_owned())
        },
        str::to_owned,
    );
    PathBuf::from(format!("{stem}.rs"))
}
