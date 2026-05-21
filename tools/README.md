# `tools/` — AI-first RPC self-verification harness

§5.49 R59 / R51.193. The Claude-side dogfood of §2 invariant #2 ("RPC
headless as AI primary path") and §2 invariant #7 ("scene-as-data"):
every visual round should end with a `tools/demos/*.py` that proves the
change by typed RPC, not by asking the human reader to describe a
screenshot.

## Quick start

```sh
cargo build --release -p hello-toggle
python3 tools/demos/hello_toggle_activate.py
# [demo] hello-toggle activate cycle
# [demo] PASS (0.87s)
```

Exit code 0 = every assertion satisfied. Non-zero = typed reason on
stderr (assertion / RPC error / unexpected exception).

## Architecture

`rpc_verify.RpcSubprocess` is a context manager that:

1. Spawns the requested `pinion-shell` example. Prefers
   `target/release/<example>` when present; falls back to
   `cargo run -p <example> --release --quiet`.
2. Pipes stdin / stdout / stderr. Drains stdout into a thread-safe
   queue and stderr into a tail buffer for diagnostics.
3. Sleeps `boot_grace` seconds (default 0.8) so winit / vello finish
   surface creation before the first request lands. The shell's
   stdin reader is already running on spawn, but RPC dispatch only
   resolves once `AppShell::resumed` completes.
4. Exposes typed wrappers:
   - `query(path) -> IntrospectValue`
   - `invoke(path, args) -> result`
   - `snapshot(path="") -> SnapshotNode`
   - `intents() -> list[Intent]`
   - `request(method, params=None)` — generic envelope.
5. Cleans up on context exit: closes stdin, `SIGTERM`, 2s wait,
   `SIGKILL` fallback.

JSON-RPC 2.0 framing is newline-delimited per
`pinion-shell::spawn_stdin_rpc_reader` — one request per line in,
one response per line out. Errors raise `RpcError(code, message, data)`
so demos can pattern-match on JSON-RPC error codes (`-32601` method
not found, `-32602` invalid params, …).

## Why Python, not Rust

The Rust workspace has `pinion-rpc` unit tests that drive an in-process
`dispatch()` — those cover the dispatcher's typed handlers exhaustively
and are the right home for permanent regressions. This harness covers
the *other* axis: the AI client driving a *real* shell over the wire,
boot grace included, with the same JSON envelope an external agent
would emit. Rust integration tests that spawn `cargo run` would
duplicate cargo dependency-resolution work on every run; a Python
script reuses the cached `target/release/` artefact and stays out of
the build graph.

Python 3.9+ stdlib only. No third-party deps. Run from the workspace
root.

## Carry list (R51.194+)

The first slice (R51.193) lands the harness primitive plus one demo
(`hello_toggle_activate.py`). The remaining RPC surface gaps that
block richer demos:

- **R51.194 — `scene/snapshot` Container / Scroll traversal.** The v0
  dispatcher only dumps the scene-root primitive's discriminator plus
  any root-`External` introspect fields. `Scene::Container`, `Scene::Scroll`,
  and `Scene::Box` children fall through to `SnapshotNode::Unknown`
  (snapshot.rs:104). Until this lands, the harness cannot enumerate
  widget rows nested under a container — and `hello-listbox` is exactly
  that shape.
- **R51.195 — wheel / key event injection RPC method.** The current
  surface has no way to synthesise a `WheelDelta` or `KeyboardEvent`
  through the wire. Demo coverage for the §5.45 Scroll axis requires
  this method so a Claude-driven demo can scroll `hello-listbox`,
  observe `ScrollState`, and assert the visible row window without a
  human at the keyboard.
- **R51.196 — `scene/click` v1: Container traversal + real event
  pipeline.** Today `scene/click` is probe-only — it reports the root
  `External`'s `handles_event` policy verdict but does not actually
  mutate state and does not descend through `Scene::Container`
  (click.rs:7-17). Once §5.3 settles a richer scene-path syntax, the
  click handler should walk to the tagged target and feed a real
  `PointerEvent` through the shell's `InputRouter`. Until then, demos
  must drive state through `scene/invoke "/external/send"` (R17
  bidirectional channel) rather than synthesised clicks.

Each of those carries is the textbook substrate-first fix for a
specific class of visual-state RPC introspection that the harness
cannot yet cover. Land them in priority order so the harness grows
along with the visible widget catalogue.
