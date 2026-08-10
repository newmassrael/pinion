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

## Headless / deterministic sweep (R720)

By default a demo spawns its windowed winit + wgpu shell against
whatever `DISPLAY` is set — i.e. the developer's physical X server
(`:0`). That borrows the host's *physical cursor*, and a window the WM
maps under it receives a spurious boot `CursorMoved` → boot-time hover
flakiness (the R719 root cause, which `scene/pointer_leave` could only
mask at the boot instant).

`tools/sweep_headless.sh` runs the whole `tools/demos/*.py` suite inside a
single throw-away **Xvfb** display, which has no physical cursor at all —
the complete fix, not a boot-instant patch:

```sh
tools/sweep_headless.sh                 # run all demos headless
tools/sweep_headless.sh r719 r697       # only demos whose name matches a substring
```

The wrapper pins `WGPU_BACKEND=gl LIBGL_ALWAYS_SOFTWARE=1` (GL/llvmpipe):
a windowed surface under lavapipe (Vulkan) panics `Out of Memory` on the
Xvfb framebuffer, while the software-GL path creates it fine. This is
test-harness env-selection only — **framework code is untouched** (no
RPC-input-only test mode); the binaries are the same shells an end user
runs. The surfaceless `headless_screenshot.rs` (R637) path is separate;
these are real windowed shells, including the live-pixel
`PINION_SCREENSHOT` demos (all pass under GL/llvmpipe).

The R719 boot `pointer_leave` baseline is retained as belt-and-suspenders
for runs against a real display.

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

## Censuses that sit beside the harness

Not every tool here drives a shell. Two count something about the tree and
answer a question a round would otherwise estimate:

- `assembled_state.py` (R1615) — how many bindings paint from state assembled
  out of **more than one external**, and how many of those publish the
  assembly. `--verbose` prints the per-binding rows and every tag argument it
  could not resolve; `--selftest` runs the scan's own cases, including the two
  it was measurably wrong about first (a direct-form-only scan misses the
  binding that reads its sibling through a typed reader, and the check that the
  typed-reader list is not short first counted four rustdoc mentions).
- `reference_names.py`, `reference_census.py`, `phase_b_tally.py` — the
  standing gates; each has its own `--selftest` and the push hook runs them.
- `pin_sync.py` (R1641) — the rev pin shared with the consumer repos that
  git-pin pinion. `--check-dual-pin` is the push gate and reads pinion alone:
  `Cargo.toml`'s SCE rev, this repo's `Cargo.lock`, and the `vendor/sce`
  submodule gitlink are three statements of one fact, and drifting them apart
  splits the SCE instance in every consumer. `--check` additionally verifies
  that the repos in `pin-consumers.txt` name one pinion rev, derive their SCE
  rev from it, and have locks that agree with their manifests; it is a human's
  command, not a gate, because a machine without those clones cannot answer it.
  Passing a rev (or `--head`) REWRITES those manifests — see the module
  docstring on why an agent session must not do that.

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

## Status

The R51.193 harness primitive and its R51.194-196 carries (snapshot
Container/Scroll traversal, wheel/key injection, `scene/click` v1 real
event pipeline) all landed long ago; the RPC surface the demos drive is
recorded in the Mnemosyne atomic store (read via `mnemosyne-cli query`).
The live source of truth for the demo
suite and per-round verification obligations is `docs/SEED_PROMPT.md`
(round log + carry list) and `git log`.
