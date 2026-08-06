//! R1574.2 §5.7 §2 #6 — the terminal backend's `scene/subscribe` contract, in a
//! test binary **of its own**.
//!
//! # Why this is a separate file
//!
//! `pinion_rpc::process_registry()` is what its name says: **process**-global.
//! This test subscribes to it and then asserts the stream's `delivered_count`,
//! and `cargo test` runs a test binary's tests in **parallel threads of one
//! process** — so every sibling test in `rpc_ingress.rs` that advances the scene
//! revision (a click, a key, an invoke, a focus set, a tick: most of the
//! twenty-three) delivers to this armed subscription too.
//!
//! That is not a hypothesis. CI run `31066749248` failed here with
//! `delivered_count` `Some(2)` where `Some(1)` was expected, and the flake
//! reproduces locally at about **1 run in 8**. The counter was reading other
//! tests' work.
//!
//! Cargo compiles every `tests/*.rs` into its own binary and therefore its own
//! process, which makes the interference **structurally impossible** rather than
//! statistically unlikely — no `--test-threads=1`, no serial-test dependency,
//! and no assertion weakened to a delta that would still race.
//!
//! The class was audited rather than assumed: `pinion-rpc`'s own twelve
//! subscription tests each build a private `SubscriptionRegistry::new()`, so
//! this was the only site in the tree reading the global one from a shared
//! binary.

use pinion_core::test_fixtures::ButtonFixture as TestButtonView;
use pinion_tui::ShellCoreTui;

// ─── R1552 §5.7 §2 #6 PINION-PR83 — change streams on the terminal backend ───

/// A recording egress: keeps every frame written to it, so a test can assert
/// what the server said *unprompted*.
///
/// The TUI's production egress writes to stderr; what matters for the contract
/// is that a frame reaches the connection's writer, so the test substitutes its
/// own rather than parsing a stream it does not own.
#[derive(Default)]
struct RecordingEgress {
    frames: std::sync::Mutex<Vec<String>>,
}

impl pinion_rpc::RpcEgress for RecordingEgress {
    fn send_frame(&self, frame: String) -> bool {
        self.frames.lock().expect("egress lock").push(frame);
        true
    }
}

/// R1552 §5.7 §2 #6 — `scene/subscribe` works on the terminal backend, the
/// stream is delivered by the same `SceneRevision` observer the GUI installs,
/// and a stream is silent until the dispatch site has ARMED it.
///
/// The §2 #6 claim this round makes is that both backends answer the same wire
/// the same way. R1552's demo proves the GUI half against a live binary; this
/// is the TUI half, and without it the claim would rest on the wiring looking
/// right rather than on it having been run.
///
/// Arming is deliberately the *caller's* step, not the core's: it must happen
/// AFTER the subscribing frame's response has been written, and only the
/// caller knows when that is (`drain_rpc_into_substrate` on this backend,
/// `AppShell::dispatch_rpc` on the GUI). Driving the core directly, as this
/// test does, is therefore what makes the un-armed window observable — so the
/// test asserts the property rather than working around it.
#[test]
fn r1552_a_terminal_connection_is_written_to_unprompted() {
    let mut core: ShellCoreTui<TestButtonView> = ShellCoreTui::new();
    // Two handles to ONE egress: the typed one the test reads, and the trait
    // object the dispatcher takes. An `Arc<RecordingEgress>` coerces to the
    // trait object on clone, so there is no downcast and no second sink.
    let recorder = std::sync::Arc::new(RecordingEgress::default());
    let egress: std::sync::Arc<dyn pinion_rpc::RpcEgress> = recorder.clone();
    let conn = pinion_rpc::ConnId::allocate();

    // Subscribing answers ONCE, like any other method.
    let response = core
        .dispatch_rpc_from(
            r#"{"jsonrpc":"2.0","id":1,"method":"scene/subscribe"}"#,
            Some((conn, &egress)),
        )
        .expect("scene/subscribe must produce a response");
    let opened: serde_json::Value = serde_json::from_str(&response).expect("valid JSON");
    let subscription = opened["result"]["subscription"]
        .as_u64()
        .expect("a subscription id");

    let delivered = |id: u64| -> Option<u64> {
        pinion_rpc::process_registry()
            .views()
            .subscriptions
            .iter()
            .find(|s| s.subscription == id)
            .map(|s| s.delivered_count)
    };

    // Un-armed: a scene advance reaches this stream with NOTHING. That is the
    // window a client would otherwise be told about an id it has not received.
    let _ = core.dispatch_rpc_from(
        r#"{"jsonrpc":"2.0","id":2,"method":"scene/tick","params":{"dt":0.016}}"#,
        Some((conn, &egress)),
    );
    assert_eq!(
        delivered(subscription),
        Some(0),
        "un-armed streams are silent"
    );
    assert!(
        recorder.frames.lock().expect("egress lock").is_empty(),
        "and nothing was written to the connection",
    );

    // What the drain does once the subscribing frame's response is on the wire.
    assert_eq!(pinion_rpc::process_registry().arm_pending(), 1);

    // Now the observer publishes: the advance already owed, named by revision.
    let _ = core.dispatch_rpc_from(
        r#"{"jsonrpc":"2.0","id":3,"method":"scene/tick","params":{"dt":0.016}}"#,
        Some((conn, &egress)),
    );
    assert_eq!(
        delivered(subscription),
        Some(1),
        "the advance reached the stream"
    );

    // And what reached the connection is a NOTIFICATION — the property the
    // whole wire form turns on, asserted on the terminal side too.
    let frames = recorder.frames.lock().expect("egress lock").clone();
    let note: serde_json::Value = serde_json::from_str(
        frames
            .last()
            .expect("the connection was written to unprompted"),
    )
    .expect("valid JSON");
    assert_eq!(note["method"], "scene/changed");
    assert_eq!(note["params"]["subscription"], subscription);
    assert_eq!(note["params"]["revision"], core.revision());
    assert!(note.get("id").is_none(), "a notification carries no id");

    // Clean up the process-wide registry so a later test in this binary starts
    // from the same state this one did.
    assert_eq!(pinion_rpc::process_registry().close_connection(conn), 1);
}
