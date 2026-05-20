//! §5.23 `Handler` trait + [`HandlerRegistry`] — async dispatch surface
//! for [`pinion_core::Command`] queued by Owner-tied reactive scopes
//! (R51.139 `Command` substrate) and reducer fallout (carry: `Update`
//! signature evolution).
//!
//! ## What lands at R51.141 (first-cut)
//!
//! - [`Handler`] trait — the R27 `async fn handle(Command) -> Intent`
//!   contract, expressed via [`HandlerFuture`] so the trait stays
//!   object-safe + the registry can store heterogeneous handlers.
//! - [`HandlerRegistry`] — boot-time `kind` → handler map (`BTreeMap`
//!   over [`Cow<'static, str>`](std::borrow::Cow) keys). Registration is
//!   swappable per R27: re-`register` the same `kind` to replace the
//!   prior handler. The registry itself is sync (`Send + Sync`); the
//!   futures it constructs cross to whichever async runtime the
//!   executor provides.
//! - Dispatch helper: [`HandlerRegistry::dispatch`] looks up the
//!   handler for [`Command::kind`](pinion_core::Command::kind) and
//!   hands back the constructed future. The caller drives it on its
//!   own executor — `pinion-runtime` stays runtime-agnostic per §6.3
//!   (view-fn sync, IO async at the boundary).
//!
//! ## What carries (R51.142+)
//!
//! - **Executor binding** — `pinion-rpc` / `pinion-shell` ties the
//!   future stream to a `tokio` (or other) runtime. The `Command`
//!   queue drain pump that polls
//!   [`Owner::take_pending_commands`](pinion_core::reactive::Owner::take_pending_commands)
//!   and feeds the registry sits there, not here.
//! - **In-flight cancellation** — Solid's "new Command from the same
//!   scope cancels the prior in-flight one" pattern (R27). Needs
//!   `JoinHandle` / `CancellationToken` plumbing that lives with the
//!   executor.
//! - **`scene/commands` RPC method** — the 10th `pinion-rpc` typed
//!   method exposes the pending queue snapshot for AI inspection
//!   (§5.7).
//! - **`Update(&mut Model, Intent) -> Vec<Command>` reducer signature
//!   evolution** + SCE schema + Forge codegen for command tables /
//!   handler bindings (§2 #8).
//!
//! ## Why `BoxFuture` instead of `impl Future`?
//!
//! The registry must store handlers heterogeneously (different
//! `kind`s → different handler implementations) behind one trait
//! object. An `impl Future` return type leaks the concrete future
//! into the caller's type, which a `dyn Handler` cannot do. Pinning
//! the future into `Box<dyn Future<Output = Intent> + Send + 'static>`
//! erases the concrete future type at the trait surface, paying one
//! allocation per dispatch in exchange for swappable storage — the
//! textbook trade for an async dispatch registry (the same shape
//! `async-trait` ends up emitting and that `tower::Service`
//! consumers wrap).

pub mod handler;
pub mod registry;

pub use handler::{Handler, HandlerFuture};
pub use registry::HandlerRegistry;
