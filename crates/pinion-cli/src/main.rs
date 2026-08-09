//! `pinion` — developer CLI entry point.
//!
//! R634 §5.7 — first land of the CLI body (pre-R634 was an empty
//! `fn main() {}` stub). The CLI is organized as a `clap` derive
//! root with one sub-command per developer workflow; this commit
//! lands the `the design tool-verify` sub-command that drives the
//! design tool → pinion design-parity verification loop documented in
//! [`figma_verify`].
//!
//! ## Sync at the CLI boundary
//!
//! The CLI binary itself stays sync per the §6.3 framework
//! boundary (view-fn purity invariant). The HTTP client is `ureq`
//! (blocking, no tokio runtime); pinion-rpc's async dispatch
//! never reaches here because the CLI does not host an RPC
//! server — it consumes external services (the design tool REST API)
//! one-shot.

mod figma_diff;
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
// R634 → R638 sub-commands all live under the design tool → pinion design-
// parity workflow (Verify spec / FetchImage reference / Diff pixels),
// so every variant naturally shares the `the design tool` prefix. The lint
// recommends de-duplication via glob import, but `clap::Subcommand`
// derives the CLI sub-command name from the variant identifier verbatim
// — stripping the prefix would rename every sub-command (e.g. `pinion
// verify` instead of `pinion the design tool-verify`), an externally-visible
// CLI surface break for a stylistic gain. Allow with the rationale
// pinned to the variant set.
#[allow(
    clippy::enum_variant_names,
    reason = "clap derives the sub-command CLI name from the variant id; renaming would break the externally-visible `pinion figma-*` surface"
)]
enum Command {
    /// R634 §5.7 — fetch a design tool file's node tree via the REST API
    /// and dump the JSON to stdout (or `--output` path). The
    /// starting point for the design tool → pinion design-parity loop;
    /// R635 added the first hand-written `Scene` binding
    /// (`examples/the design tool-button-m3`).
    FigmaVerify(figma_verify::FigmaVerifyArgs),

    /// R636 §5.7 — export a design tool node as a reference PNG (or
    /// SVG / JPG / PDF) so the pinion-side
    /// [`scene/screenshot`](pinion_rpc::screenshot) can be
    /// pixel-diffed against the design tool original. Two-leg fetch:
    /// image-list endpoint → per-node S3 URL → PNG bytes.
    FigmaFetchImage(figma_image::FigmaImageArgs),

    /// R638 §5.7 — pixel-diff two PNGs (typically pinion-rendered
    /// vs the design tool reference, R637 + R636 output) with per-channel MAE
    /// / max-delta / exact-match metrics + optional diff
    /// visualization PNG. Closes the design tool → pinion design-parity
    /// verification loop.
    FigmaDiff(figma_diff::FigmaDiffArgs),
}

fn main() {
    let cli = Cli::parse();
    let result = match &cli.command {
        Command::FigmaVerify(args) => figma_verify::run(args),
        Command::FigmaFetchImage(args) => figma_image::run(args),
        Command::FigmaDiff(args) => figma_diff::run(args),
    };
    if let Err(err) = result {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
