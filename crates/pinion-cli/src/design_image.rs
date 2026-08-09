//! R636 §5.7 — `pinion design-fetch-image` sub-command.
//!
//! Reference-PNG side of the design tool → pinion design-parity loop. Pre-R636
//! the workflow stalled after `design-verify` JSON fetch — comparing pinion's `scene/screenshot` PNG
//! against the original the design tool rendering required manually opening
//! the design tool and exporting each frame. R636 automates the design tool
//! side: one CLI call, per-node PNG saved to disk, ready for pixel-diff
//! (R637+).
//!
//! ## Two-step the design tool API contract
//!
//! Unlike the file endpoint (R634), the design tool's image endpoint does not
//! return PNG bytes directly. The flow is:
//!
//! 1. `GET /v1/images/:file_key?ids=:nodes&format=png&scale=:scale`
//!    → JSON containing per-node S3 URLs (the URLs expire after ~30
//!    minutes per the design tool's documented contract)
//! 2. `GET <s3_url>` for each node → actual PNG bytes
//!
//! This sub-command implements both legs for a single node id and
//! writes the PNG to `--output`.
//!
//! ## Wire shape
//!
//! ```text
//! $ pinion design-fetch-image FILE_KEY 51553:5180 --output /tmp/btn.png
//! wrote 4827 bytes to /tmp/btn.png
//! ```
//!
//! ## Format / scale
//!
//! - `--format png` (default) / `jpg` / `svg` / `pdf` per the design tool
//!   `?format=` documentation
//! - `--scale 1.0` (default) / `2.0` / `0.5` — multiplier on the
//!   node's natural size; `2.0` for retina-density reference, `0.5`
//!   for thumbnail
//!
//! ## Authentication
//!
//! Reuses the auth-token env var contract from R634; only the
//! image-list endpoint needs the header, the per-node S3 URLs are
//! pre-signed and require no auth.

use std::io::Read;
use std::path::PathBuf;

use crate::design_api;

use clap::Args;

/// Arguments for the `design-fetch-image` sub-command.
#[derive(Args)]
pub struct DesignImageArgs {
    /// The design tool file key — same URL slot as `design-verify` (R634).
    pub file_key: String,

    /// Single the design tool node id to export. Use the colon form
    /// (`51553:5180`), not the URL hyphen form (`51553-5180`).
    pub node_id: String,

    /// Output path for the PNG bytes. Required (unlike
    /// `design-verify` which defaults to stdout) because piping
    /// binary PNG to a terminal would corrupt the bytes.
    #[arg(short, long)]
    pub output: PathBuf,

    /// Multiplier on the node's natural size. `1.0` exports the
    /// node at the same dimensions the design tool displays; `2.0` doubles
    /// for retina reference; `0.5` halves for thumbnails. The design tool
    /// caps the result at 16 megapixels per node.
    #[arg(long, default_value_t = 1.0)]
    pub scale: f32,

    /// Output format — `png` (default) / `jpg` / `svg` / `pdf` per
    /// the design tool image-endpoint contract.
    #[arg(long, default_value = "png")]
    pub format: String,
}

/// R636 §5.7 — execute the `design-fetch-image` sub-command.
///
/// # Errors
///
/// - the auth token env var not set (same contract as R634)
/// - the design tool image-list endpoint HTTP error
/// - the design tool response missing the requested node id in the `images`
///   map (node id typo, or the node is invisible / un-exportable)
/// - S3 PNG fetch HTTP error (URL expired — retry the whole
///   command, the URL TTL is ~30 minutes)
/// - I/O error writing to `--output`
pub fn run(args: &DesignImageArgs) -> Result<(), Box<dyn std::error::Error>> {
    let token = design_api::token()?;

    // Step 1 — request the per-node image URL via the official
    // image-list endpoint. The response is a JSON object whose
    // `images` field maps node id → pre-signed S3 URL.
    let url = format!(
        "{}/images/{}?ids={}&format={}&scale={}",
        design_api::API_HOST,
        args.file_key,
        args.node_id,
        args.format,
        args.scale,
    );
    let response = ureq::get(&url)
        .set(design_api::TOKEN_HEADER, &token)
        .call()
        .map_err(|err| format!("image-list request failed: {err}"))?;
    let payload: serde_json::Value = response
        .into_json()
        .map_err(|err| format!("image-list response is not valid JSON: {err}"))?;

    // The design tool contract reports per-call failures in a top-level
    // `err` field; non-null means the whole request failed even if
    // the HTTP status was 200. Surface verbatim so the user can
    // adjust their query.
    if let Some(err_msg) = payload.get("err").and_then(serde_json::Value::as_str) {
        return Err(format!("image-list returned error: {err_msg}").into());
    }

    let images = payload
        .get("images")
        .and_then(serde_json::Value::as_object)
        .ok_or("image-list response missing 'images' object")?;
    let image_url = images
        .get(&args.node_id)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            format!(
                "image-list response has no URL for node {} \
                 — verify the node id is exportable and visible",
                args.node_id,
            )
        })?;

    // Step 2 — download the actual PNG bytes from the pre-signed
    // S3 URL. No auth header (URL is pre-signed) and the response
    // body is binary, so `into_reader().read_to_end` is the
    // canonical ureq idiom for raw bytes (vs `into_json` /
    // `into_string` which assume text).
    let png_response = ureq::get(image_url)
        .call()
        .map_err(|err| format!("image blob fetch failed: {err}"))?;
    let mut bytes = Vec::new();
    png_response
        .into_reader()
        .read_to_end(&mut bytes)
        .map_err(|err| format!("image blob read failed: {err}"))?;

    let display = args.output.display();
    std::fs::write(&args.output, &bytes)
        .map_err(|err| format!("write to {display} failed: {err}"))?;
    eprintln!("wrote {} bytes to {}", bytes.len(), display);

    Ok(())
}
