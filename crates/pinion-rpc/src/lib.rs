//! pinion-rpc — JSON-RPC 2.0 server with typed-hybrid method shape (§5.7).
//!
//! Path resolution against the SCE-emitted window topology lives in
//! [`path`] per §5.18 (optional `/window[id]/` prefix with single-window
//! short-circuit). Per-method dispatchers ([`query`], [`click`],
//! [`rewind`], [`snapshot`], [`dry_run`], [`wait_for`], [`screenshot`],
//! [`invoke`]; §5.12 ratified 7, R17 bidirectional-RPC spec round
//! extended to 8) each live in their own module. The JSON-RPC 2.0
//! wire envelope and method routing entry point live in [`dispatch`].

pub mod click;
pub mod dispatch;
pub mod dry_run;
pub mod invoke;
pub mod path;
pub mod query;
pub mod rewind;
pub mod screenshot;
pub mod snapshot;
pub mod wait_for;

pub use click::{click, ClickError, ClickOutcome};
pub use dispatch::{dispatch, Request, RequestId, Response, RpcError};
pub use dry_run::{dry_run, DryRunError};
pub use invoke::{invoke, InvokeError};
pub use path::{resolve, PathError, ResolvedPath};
pub use query::{query, QueryError};
pub use rewind::{rewind, RewindError};
pub use screenshot::{screenshot, Screenshot, ScreenshotError};
pub use snapshot::{snapshot, ExternalSnapshot, SnapshotError, SnapshotNode};
pub use wait_for::{wait_for, WaitForError, WaitOutcome};
