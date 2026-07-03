//! §5.23 [`Handler`] trait + [`HandlerRegistry`] + [`Executor`] +
//! [`IntentSink`] + [`CommandExecutor`] — full async dispatch surface
//! for [`pinion_core::Command`] queued by Owner-tied reactive scopes
//! (R51.139 [`Command`](pinion_core::Command) substrate) and reducer fallout (carry:
//! `Update` signature evolution).
//!
//! ## What lands at R51.141 (first-cut Handler + Registry)
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
//! ## What lands at R51.156 (this round — executor binding substrate)
//!
//! - [`Executor`] trait + [`BoxFuture`] alias + [`CommandTaskHandle`]
//!   cancel handle. The substrate-side async runtime abstraction;
//!   concrete impls (`pinion-shell` tokio current-thread, `pinion-tui`
//!   tokio current-thread, `pinion-rpc` async dispatch) live at the
//!   backend boundary.
//! - [`IntentSink`] trait — the wake surface a resolved [`Intent`](pinion_core::Intent)
//!   travels through back to the UI thread.
//! - [`CommandExecutor`] composite — `(registry + executor + sink)`
//!   bundle. [`CommandExecutor::dispatch`] looks up the handler, wraps
//!   the [`HandlerFuture`] so the resolved [`Intent`](pinion_core::Intent) reaches the
//!   [`IntentSink`], and spawns via [`Executor::spawn`]. Returns a
//!   [`CommandTaskHandle`] for R51.158's per-scope cancellation.
//! - [`BlockOnExecutor`] — reference [`Executor`] impl using
//!   [`futures_executor::block_on`]. Synchronous; useful for tests and
//!   for binaries that prefer sync dispatch.
//! - [`VecSink`] — test-fixture [`IntentSink`] backed by
//!   `Arc<Mutex<Vec<Intent>>>`.
//!
//! ## What carries (R51.157+)
//!
//! - **R51.157** — `CoreShell::dispatch_pending_commands` walks
//!   [`Owner::take_pending_commands_recursive`](pinion_core::reactive::Owner::take_pending_commands_recursive)
//!   and feeds each [`Command`](pinion_core::Command) to the [`CommandExecutor`]. `CoreShell`
//!   gains an optional `executor: Option<Arc<CommandExecutor>>` field
//!   so headless tests can omit it.
//! - **R51.158** — per-scope cancellation: [`CommandExecutor`] gains a
//!   `Mutex<BTreeMap<scope_id, CommandTaskHandle>>` that aborts the
//!   prior in-flight handle when a new [`Command`](pinion_core::Command) arrives on the
//!   same scope (R27 Solid pattern).
//! - **R51.159** — `pinion-shell` concrete tokio current-thread
//!   [`Executor`] + winit `EventLoopProxy<AppEvent>`-based
//!   [`IntentSink`] + `AppEvent::IntentArrived` user-event variant +
//!   `ShellCore::dispatch_intent` re-feeding intents into the
//!   [`Scene::External`](pinion_core::Scene::External) `invoke("send", _)`
//!   channel.
//! - **`scene/commands` RPC method** — the 10th `pinion-rpc` typed
//!   method exposes the pending queue snapshot for AI inspection
//!   (§5.7).
//! - **`Update(&mut Model, Intent) -> Vec<Command>` reducer signature
//!   evolution** + SCE schema + Forge codegen for command tables /
//!   handler bindings (§2 #8).
//!
//! ## Why [`BoxFuture`] instead of `impl Future`?
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

pub mod executor;
pub mod handler;
pub mod registry;
pub mod sink;

pub use executor::{BlockOnExecutor, BoxFuture, CommandExecutor, CommandTaskHandle, Executor};
pub use handler::{Handler, HandlerFuture};
pub use registry::HandlerRegistry;
pub use sink::{IntentSink, VecSink};
