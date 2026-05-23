//! `pinion` — developer CLI entry point.
//!
//! R634 §5.7 — first land of the CLI body (pre-R634 was an empty
//! `fn main() {}` stub). The CLI is organized as a `clap` derive
//! root with one sub-command per developer workflow; this commit
//! lands the `figma-verify` sub-command that drives the
//! Figma → pinion design-parity verification loop documented in
//! [`figma_verify`].
//!
//! ## Sync at the CLI boundary
//!
//! The CLI binary itself stays sync per the §6.3 framework
//! boundary (view-fn purity invariant). The HTTP client is `ureq`
//! (blocking, no tokio runtime); pinion-rpc's async dispatch
//! never reaches here because the CLI does not host an RPC
//! server — it consumes external services (Figma REST API)
//! one-shot.

mod figma_image;
mod figma_verify;

use clap::{Parser, Subcommand};

/// Developer CLI for the pinion framework. Sub-commands group
/// per-workflow tools; new sub-commands land alongside the round
/// that needs them rather than being pre-allocated.
#[derive(Parser)]
#[command(name = "pinion", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// R634 §5.7 — fetch a Figma file's node tree via the REST API
    /// and dump the JSON to stdout (or `--output` path). The
    /// starting point for the Figma → pinion design-parity loop;
    /// R635 added the first hand-written `Scene` binding
    /// (`examples/figma-button-m3`).
    FigmaVerify(figma_verify::FigmaVerifyArgs),

    /// R636 §5.7 — export a Figma node as a reference PNG (or
    /// SVG / JPG / PDF) so the pinion-side
    /// [`scene/screenshot`](pinion_rpc::screenshot) can be
    /// pixel-diffed against the Figma original. Two-leg fetch:
    /// image-list endpoint → per-node S3 URL → PNG bytes.
    FigmaFetchImage(figma_image::FigmaImageArgs),
}

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::FigmaVerify(args) => figma_verify::run(args),
        Command::FigmaFetchImage(args) => figma_image::run(args),
    };
    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
