//! Preview lifecycle ledger (§5.34 R40.1 model-first slice).
//!
//! Stateful ledger backing the `scene/propose_change` /
//! `scene/apply_preview` / `scene/cancel_preview` / `scene/list_previews`
//! RPC lifecycle (R40.2+ wires the dispatch entry points). Conflict
//! policy is the **independent ledger with optimistic concurrency
//! control** option (§5.34 Q2=C): multiple previews may anchor the
//! same target path, and conflict detection happens at apply time via
//! a per-entry base-revision token compared against the live scene
//! revision the caller supplies.
//!
//! # Layering
//!
//! The ledger lives in `pinion-rpc` because preview lifecycle is an
//! RPC-protocol concern, not a core scene model. `pinion-core` stays
//! focused on scene primitives; the ledger composes them through the
//! [`Proposal`] trait without `pinion-core` ever needing to depend on
//! the lifecycle layer.
//!
//! # Forward-compat
//!
//! [`Proposal`] is an open trait so future change kinds (`SetSignal`,
//! `ReplaceView`, `SetStyle`, `DispatchIntent` — landing in R40.5
//! per §5.34) can be added without touching the ledger surface. The
//! ledger only needs the trait's path-introspection methods.

mod apply;
mod cancel;
mod error;
mod id;
mod kinds;
mod ledger;
mod list;
mod proposal;
mod propose;

pub use apply::{apply_preview, ApplyOutcome};
pub use cancel::cancel_preview;
pub use error::{ApplyError, ProposeError};
pub use id::PreviewId;
pub use kinds::TypedProposal;
pub use ledger::{
    Entry, PreviewLedger, PreviewView, SweepReport, DEFAULT_CAPACITY, DEFAULT_TTL, MAX_TTL,
};
pub use list::list_previews;
pub use proposal::Proposal;
pub use propose::{propose_change, ProposeOutcome};
