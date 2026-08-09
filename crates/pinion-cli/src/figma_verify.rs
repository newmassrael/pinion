//! R634 §5.7 — `pinion the design tool-verify` sub-command.
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
//! Token comes from the `FIGMA_TOKEN` environment variable. Use a
//! Personal Access Token with the **File content** read scope only
//! (the design tool.com → Settings → Personal access tokens). The CLI never
//! writes the token to disk or includes it in the dump output.
//!
//! ## Wire shape
//!
//! ```text
//! $ FIGMA_TOKEN=figd_... pinion the design tool-verify <FILE_KEY>
//! { "name": "...", "document": { "id": "0:0", "type": "DOCUMENT",
//!   "children": [ { "id": "1:2", "type": "CANVAS", ... } ] }, ... }
//! ```
//!
//! `FILE_KEY` is the path segment between `/design/` (or `/file/`)
//! and the file name in a design tool URL:
//!
//! ```text
//! https://www.figma.com/design/AbCdEfGhIj/My-Design?node-id=...
//!                              ^^^^^^^^^^^^
//!                              FILE_KEY
//! ```

use std::path::PathBuf;

use clap::Args;

/// Arguments for the `the design tool-verify` sub-command.
#[derive(Args)]
pub struct FigmaVerifyArgs {
    /// The design tool file key — the URL path segment between `/design/`
    /// (or `/file/`) and the file name. Example: for
    /// `https://www.figma.com/design/AbCdEfGhIj/My-Design`, pass
    /// `AbCdEfGhIj`.
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

/// R634 §5.7 — execute the `the design tool-verify` sub-command.
///
/// # Errors
///
/// - `FIGMA_TOKEN` environment variable not set (the only auth
///   source — the CLI deliberately does not read from a config
///   file to avoid accidentally committing a token).
/// - the design tool API HTTP error (4xx / 5xx) — the response body is
///   echoed to stderr for debugging.
/// - I/O error writing to `--output` path.
/// - JSON serialization error pretty-printing the response.
pub fn run(args: &FigmaVerifyArgs) -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("FIGMA_TOKEN").map_err(|_| {
        "FIGMA_TOKEN environment variable not set; \
         export FIGMA_TOKEN=<personal access token> first \
         (figma.com → Settings → Personal access tokens, \
         File content read scope)"
    })?;

    // R634 §5.7 — the design tool official REST API endpoint. The
    // documentation lives at `the design tool.com/developers/api#files-endpoints`; the response shape is the full DocumentNode
    // tree with every FRAME / GROUP / RECTANGLE / TEXT / VECTOR / etc. typed.
    let mut url = format!("https://api.figma.com/v1/files/{}", args.file_key);
    if let Some(ids) = args.ids.as_deref() {
        // The `ids` query parameter restricts the response to the
        // listed subtree ids — useful for large UI kit files where
        // only one canvas is the reference.
        url.push_str("?ids=");
        url.push_str(ids);
    }

    let response = ureq::get(&url)
        .set("X-Figma-Token", &token)
        .call()
        .map_err(|err| format!("Figma API request failed: {err}"))?;

    let json: serde_json::Value = response
        .into_json()
        .map_err(|err| format!("Figma API response is not valid JSON: {err}"))?;

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
