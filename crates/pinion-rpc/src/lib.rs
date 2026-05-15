//! pinion-rpc — JSON-RPC 2.0 server with typed-hybrid method shape (§5.7).
//!
//! Path resolution against the SCE-emitted window topology lives in
//! [`path`] per §5.18 (optional `/window[id]/` prefix with single-window
//! short-circuit). Per-method dispatchers ([`query`], [`click`],
//! [`rewind`], [`snapshot`], [`dry_run`], …; §5.12 defines 7 in total)
//! live in their own modules. The JSON-RPC 2.0 wire envelope and method
//! routing entry point live in [`dispatch`].

pub mod click;
pub mod dispatch;
pub mod dry_run;
pub mod path;
pub mod query;
pub mod rewind;
pub mod snapshot;

pub use click::{click, ClickError, ClickOutcome};
pub use dispatch::{dispatch, Request, RequestId, Response, RpcError};
pub use dry_run::{dry_run, DryRunError};
pub use path::{resolve, PathError, ResolvedPath};
pub use query::{query, QueryError};
pub use rewind::{rewind, RewindError};
pub use snapshot::{snapshot, ExternalSnapshot, SnapshotError, SnapshotNode};
