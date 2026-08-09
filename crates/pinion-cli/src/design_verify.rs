//! R634 §5.7 — `pinion design-verify` sub-command.
//!
//! Fetches a design tool file's full node tree via the official REST
//! API and emits the response JSON to stdout (or `--output`
//! path). The starting point of the design tool → pinion design-parity
//! verification loop:
//!
//! 1. R634 — fetch + dump (this commit)
//! 2. R635 — JSON → `Scene` mapping pass (`FRAME` → [`ContainerNode`],
//!    `RECTANGLE` → [`BoxNode`], `TEXT` → [`TextNode`], `fill` /
//!    `stroke` / `cornerRadius` transcribe)
//! 3. R636+ — `scene/screenshot` RPC + the design tool image API export →
//!    per-pixel diff; substrate gaps (gradient / drop shadow /
//!    per-corner radius / etc.) feed back into the R-round queue
//!
//! [`ContainerNode`]: pinion_core::scene::ContainerNode
//! [`BoxNode`]: pinion_core::scene::BoxNode
//! [`TextNode`]: pinion_core::scene::TextNode
//!
//! ## Authentication
//!
//! Token comes from the environment variable [`design_api::TOKEN_ENV`] names.
//! Use a Personal Access Token with the **File content** read scope only. The
//! CLI never writes the token to disk or includes it in the dump output.
//!
//! ## Wire shape
//!
//! ```text
//! $ <TOKEN_ENV>=figd_... pinion design-verify <FILE_KEY>
//! { "name": "...", "document": { "id": "0:0", "type": "DOCUMENT",
//!   "children": [ { "id": "1:2", "type": "CANVAS", ... } ] }, ... }
//! ```
//!
//! `FILE_KEY` is the path segment between `/design/` (or `/file/`)
//! and the file name in a design tool URL:
//!
//! ```text
//! <service>/design/AbCdEfGhIj/My-Design?node-id=...
//!            ^^^^^^^^^^^^
//!            FILE_KEY
//! ```

use std::path::PathBuf;

use crate::design_api;

use clap::Args;

/// Arguments for the `design-verify` sub-command.
#[derive(Args)]
pub struct DesignVerifyArgs {
    /// The design tool file key — the URL path segment between `/design/`
    /// (or `/file/`) and the file name. For the example URL
    /// [`design_api::FILE_URL_EXAMPLE`] holds, that is `AbCdEfGhIj`.
    pub file_key: String,

    /// Optional output path for the dumped JSON. Defaults to
    /// stdout when omitted; useful for piping to `jq` or saving
    /// for downstream R635 Scene mapping.
    #[arg(short, long)]
    pub output: Option<PathBuf>,

    /// Optional comma-separated node id list — if supplied, the
    /// design tool API restricts the response to those subtree ids
    /// instead of the full document. Useful when the reference
    /// design is one CANVAS frame inside a larger UI kit file.
    /// Example: `--ids '1:2,1:5'`.
    #[arg(long)]
    pub ids: Option<String>,
}

/// R634 §5.7 — execute the `design-verify` sub-command.
///
/// # Errors
///
/// - the auth token environment variable not set (the only auth
///   source — the CLI deliberately does not read from a config
///   file to avoid accidentally committing a token).
/// - the design tool API HTTP error (4xx / 5xx) — the response body is
///   echoed to stderr for debugging.
/// - I/O error writing to `--output` path.
/// - JSON serialization error pretty-printing the response.
pub fn run(args: &DesignVerifyArgs) -> Result<(), Box<dyn std::error::Error>> {
    // A person reaching for this has the file OPEN, so the thing under their
    // cursor is the URL, not the key inside it. Pasting the whole URL used to
    // reach the service as a file key and come back as a flat 404; saying
    // which segment to take is the difference between a dead end and a fix.
    if args.file_key.contains("://") || args.file_key.contains('/') {
        return Err(format!(
            "`{}` looks like a URL, not a file key. The key is the path \
             segment between `/design/` (or `/file/`) and the file name: in \
             `{}` it is `AbCdEfGhIj`.",
            args.file_key,
            design_api::FILE_URL_EXAMPLE,
        )
        .into());
    }

    let token = design_api::token()?;

    // R634 §5.7 — the design service's own files endpoint. The response shape
    // is the full document-node tree with every FRAME / GROUP / RECTANGLE /
    // TEXT / VECTOR / etc. typed.
    let mut url = format!("{}/files/{}", design_api::API_HOST, args.file_key);
    if let Some(ids) = args.ids.as_deref() {
        // The `ids` query parameter restricts the response to the
        // listed subtree ids — useful for large UI kit files where
        // only one canvas is the reference.
        url.push_str("?ids=");
        url.push_str(ids);
    }

    let response = ureq::get(&url)
        .set(design_api::TOKEN_HEADER, &token)
        .call()
        .map_err(|err| format!("design API request failed: {err}"))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|err| format!("design API response is not valid JSON: {err}"))?;

    let pretty = serde_json::to_string_pretty(&json)
        .map_err(|err| format!("JSON pretty-print failed: {err}"))?;

    match args.output.as_ref() {
        Some(path) => {
            let display = path.display();
            std::fs::write(path, &pretty)
                .map_err(|err| format!("write to {display} failed: {err}"))?;
            eprintln!("wrote {} bytes to {}", pretty.len(), display);
        }
        None => {
            println!("{pretty}");
        }
    }

    Ok(())
}
