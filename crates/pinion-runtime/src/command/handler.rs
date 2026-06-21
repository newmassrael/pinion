//! [`Handler`] trait + [`HandlerFuture`] boxed return alias.
//!
//! R27 contract: `async fn handle(Command) -> Intent`. In stable Rust
//! today the object-safe expression is "return a pinned, boxed,
//! send-able future of `Intent`" — see the module doc for why we erase
//! the concrete future type at the trait surface.

use core::future::Future;
use core::pin::Pin;

use pinion_core::{Command, Intent};

/// Boxed return future from a [`Handler::handle`] call.
///
/// `Pin<Box<dyn Future<Output = Intent> + Send + 'static>>`:
///
/// - `Pin<Box<...>>` lets the registry store and pass the future
///   around as an owned value; the future itself is the only resource
///   pinned in memory.
/// - `Send` so the future can be polled on a multi-thread executor
///   (`tokio::spawn` on the multi-thread runtime requires it).
/// - `'static` so the future has no borrowed dependencies on the
///   registry / handler — once produced, it owns whatever state it
///   needs.
pub type HandlerFuture = Pin<Box<dyn Future<Output = Intent> + Send + 'static>>;

/// Async dispatch surface for [`Command`] kinds.
///
/// Per §5.23 R27 the contract is `async fn handle(Command) -> Intent`.
/// Stored as `Arc<dyn Handler>` inside
/// [`HandlerRegistry`](crate::command::HandlerRegistry), so we require:
///
/// - `Send + Sync` — handlers may be cloned to and called from worker
///   threads when a multi-thread executor drives the futures.
/// - `'static` — the registry itself outlives any borrow that the
///   handler would otherwise need.
///
/// ### Implementing
///
/// A handler is normally a small struct that captures the external
/// resources its kind needs (HTTP client, audio device handle, ...)
/// and constructs a future per call. For tests and trivially-stateless
/// handlers, the blanket impl on `Fn(Command) -> HandlerFuture`
/// (`Send + Sync + 'static`) lets a closure stand in directly.
pub trait Handler: Send + Sync + 'static {
    fn handle(&self, command: Command) -> HandlerFuture;
}

impl<F> Handler for F
where
    F: Fn(Command) -> HandlerFuture + Send + Sync + 'static,
{
    fn handle(&self, command: Command) -> HandlerFuture {
        (self)(command)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use futures_executor::block_on;
    use pinion_core::external::IntrospectValue;
    use pinion_core::{Command, Intent};

    use super::{Handler, HandlerFuture};

    fn echo_intent(cmd: Command) -> Intent {
        Intent::new_owned(format!("echo.{}", cmd.kind_str()), cmd.payload)
    }

    #[test]
    fn closure_blanket_impl_returns_intent() {
        let handler: Arc<dyn Handler> =
            Arc::new(|cmd: Command| -> HandlerFuture { Box::pin(async move { echo_intent(cmd) }) });
        let cmd = Command::new_static("audio.play", IntrospectValue::Int(440), 1);
        let intent = block_on(handler.handle(cmd));
        assert_eq!(intent.tag_str(), "echo.audio.play");
        assert_eq!(intent.payload, IntrospectValue::Int(440));
    }

    #[test]
    fn handler_is_object_safe() {
        let handler: Box<dyn Handler> =
            Box::new(|cmd: Command| -> HandlerFuture { Box::pin(async move { echo_intent(cmd) }) });
        let cmd = Command::new_static("clipboard.write", IntrospectValue::Text("x".into()), 4);
        let intent = block_on(handler.handle(cmd));
        assert_eq!(intent.tag_str(), "echo.clipboard.write");
    }

    #[test]
    fn handler_state_is_observable_per_call() {
        struct Counter {
            calls: AtomicUsize,
        }
        impl Handler for Counter {
            fn handle(&self, command: Command) -> HandlerFuture {
                let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
                Box::pin(async move {
                    Intent::new_owned(
                        format!("counter.{n}"),
                        IntrospectValue::Int(command.scope_id.try_into().unwrap_or(i64::MAX)),
                    )
                })
            }
        }
        let counter = Arc::new(Counter {
            calls: AtomicUsize::new(0),
        });
        let cmd_a = Command::new_static("ping", IntrospectValue::Null, 11);
        let cmd_b = Command::new_static("ping", IntrospectValue::Null, 13);
        let intent_a = block_on(counter.handle(cmd_a));
        let intent_b = block_on(counter.handle(cmd_b));
        assert_eq!(intent_a.tag_str(), "counter.1");
        assert_eq!(intent_b.tag_str(), "counter.2");
        assert_eq!(intent_a.payload, IntrospectValue::Int(11));
        assert_eq!(intent_b.payload, IntrospectValue::Int(13));
    }
}
