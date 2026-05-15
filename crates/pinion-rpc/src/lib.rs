//! pinion-rpc — JSON-RPC 2.0 server with typed-hybrid method shape (§5.7).
//!
//! Path resolution against the SCE-emitted window topology lives in
//! [`path`] per §5.18 (optional `/window[id]/` prefix with single-window
//! short-circuit). Per-method dispatchers (currently only `scene/query`,
//! §5.12 item 1 of 7) live in [`query`]. The JSON-RPC 2.0 wire envelope
//! and method routing entry point live in [`dispatch`].

pub mod dispatch;
pub mod path;
pub mod query;

pub use dispatch::{dispatch, Request, RequestId, Response, RpcError};
pub use path::{resolve, PathError, ResolvedPath};
pub use query::{query, QueryError};
