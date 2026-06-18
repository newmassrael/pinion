# PR-3 spec round — pinion-shell external-async-data → repaint seam

> **Status: IMPLEMENTED — R999 (`af1f91d`) + R1000 (`23ca567`), pushed.** This
> began as a DRAFT proposal (§3–§5 below describe the proposed shape); it was
> ratified and built. **§0 records the as-built deltas from the proposal — read
> §0 first; where §3–§5 disagree with §0, §0 (and the R999/R1000 changelog) is
> authoritative.** Spec-level change to `pinion-shell` + a new boundary trait.
> Seeded by the sprag→pinion handoff's PR-3 (2026-06-18).
>
> **Revision note (post adversarial audit).** An earlier draft of this doc
> proposed `run_with_external` + `use_boot`/`BootData` + a raw
> `EventLoopProxy`-backed `WakeHandle`. That design was **rejected on
> textbook/SSOT grounds** — see §A. The design below extends pinion's existing
> §6.3 async boundary substrate by exactly one edge instead of building a
> parallel seam.

## 0. As-built (R999 `af1f91d` + R1000 `23ca567`) — deltas from the proposal

Three places where what shipped intentionally diverges from §3–§5 (so a reader
treating this as the spec-of-record is not misled):

1. **Trait home: `pinion-core/src/reactive/repaint.rs`, not `pinion-runtime/src/command/sink.rs`.**
   §3.1/§5 Q3 guessed the `IntentSink` neighbourhood. During implementation the
   dependency layering decided it: examples + bindings depend on `pinion-core`
   (not `pinion-runtime`), and `use_repaint_sink` belongs beside its sibling
   `use_local_task_pump` in the reactive layer. `RepaintSink` therefore lives in
   `pinion-core`; the shell's `ProxyRepaintSink` impl stays in
   `pinion-shell/src/executor.rs` (next to `ProxyIntentSink`).

2. **No `RepaintScope` — `fn request_repaint(&self)` and payload-free `AppEvent::ExternalRepaint`.**
   §3.1 proposed `request_repaint(&self, scope: RepaintScope)` +
   `ExternalRepaint(Option<window-id>)`. Shipped: no scope, no payload — the
   handler arms a binding-wide `request_redraw`. Per-window scope is deferred to
   a multi-window live-data 2nd consumer (`abstraction-needs-second-consumer` /
   YAGNI), an additive change when one appears.

3. **Verification (replaces §6's unchecked criteria + its "deterministic write→repaint test").**
   - `RepaintSink` mechanism: 5 deterministic `pinion-core` tests
     (`reactive/repaint.rs`), incl. a real cross-thread `request_repaint` via
     `std::thread::spawn` + `join`.
   - End-to-end data path: R1000 `hello-live-data` (8 unit tests incl. a
     cross-thread tick→append) + `tools/demos/r1000_live_data.py` under
     `PINION_HIDDEN_WINDOW` (deterministic ×3). The demo proves the data edge
     (off-thread write → shared buffer → painted scene); a `scene/snapshot`
     re-renders, so it cannot isolate the autonomous wake.
   - The `AppEvent::ExternalRepaint => request_redraw` handler arm is a
     one-liner that is NOT separately unit-tested (a live `winit::EventLoop` is
     impractical headless — the same reason `executor.rs` skips the
     `ProxyIntentSink` smoke); it is exercised transitively by the demo.

   Companion sprag wiring landed local (sprag `Round 23` `61d47f6`): the
   `sprag-terminal` reader `on_dirty` hook. The full windowed-host `WidgetView`
   binding (the §7 companion item) remains the genuine open feature.

## 1. Audit conclusion (verified against `27bc84a`)

The handoff's PR-3 blocking claims all hold, BUT its premise — "no
external-async-data → repaint seam exists" — is **partially wrong**: it
reasoned from `AppEvent` + reactive + `External` + `spawn_stdin_rpc_reader`
only, and **overlooked the §6.3 async boundary substrate**
(`Executor`/`IntentSink`/`Resource`/`LocalTaskPump`/R924). Three of the four
pieces a live-data seam needs already exist.

| Fact | Evidence |
|---|---|
| `State: Copy` cannot hold `Arc<Mutex<…>>` | `pinion-core/src/widget_core.rs:143` |
| `view()` is a static by-value fn | `pinion-core/src/widget_core.rs:266` |
| `create_external()` is a no-arg static fn | `widget_core.rs:157`; invoked `pinion-runtime/src/core_shell.rs:472` |
| reactive layer (`Signal`/`Owner`) is `!Send` (thread-local) | `pinion-core/src/reactive/owner.rs`, `signal.rs` — confirmed |
| `AppEvent` is already `pub`; variant set + proxy are closed | `pinion-shell/src/lib.rs:97`; proxy built in `run()` `app.rs:2320`, only `AppShell::new(proxy)` pub `app.rs:315` |
| **Cross-thread executor EXISTS** | `pinion-shell/src/executor.rs:76` `TokioExecutor` (multi-thread, `enable_all` ⇒ fd reactor on a worker thread) |
| **Cross-thread → UI wake EXISTS** (but Intent-typed) | `executor.rs:140` `ProxyIntentSink` → `AppEvent::IntentArrived` `app.rs:1710` |
| **Boundary-trait SSOT** (runtime-agnostic; shell supplies impl) | `pinion-runtime/src/command/executor.rs:104` `Executor`, `command/sink.rs:61` `IntentSink` |
| **Async data-layer read-in-view EXISTS** (fetch-shaped) | `Resource`/`use_local_task_pump` `reactive/resource.rs`; per-frame drain `substrate.rs:2558`; R924 keep-alive `app.rs:1829` |
| **No repaint/wake sink exists; no stream primitive exists** | only `IntentSink`; `Resource` is `fetch_with` + `DeferredReady` (request→response) — confirmed by grep |

**Verdict:** the *one* genuine gap is a **content-free "external data changed →
repaint" wake from a worker thread**, plus the **continuous-producer (stream)
async shape** that `Resource` (fetch-shaped) does not cover. Data injection and
cross-thread execution are already solved by existing substrate.

## 2. Problem statement

A terminal pane's content changes when a child process writes its PTY from a
separate thread. The grid is producer-authoritative (R969: sprag owns the
PTY+vte engine; pinion projects). pinion needs: (a) the binding to read that
shared `Send` data in `view()` each frame, (b) the writer thread to trigger a
repaint on change. This is **GUI-only** — the headless/RPC north-star already
works (`scene/snapshot` projects `Scene::TextGrid`, R972-R978.1).

## 3. Design — one new boundary edge; everything else reused

### 3.1 The single new edge: a `RepaintSink` boundary trait (R3.1)

`IntentSink::send(Intent)` is semantically *"re-feed a resolved Intent into the
SCXML send channel / reducer."* A content-free repaint is **not** a reducer
event (the grid is not `State`, R969) — overloading `IntentSink`, `IntentArrived`,
or `WindowsDirty` for it is the smell the handoff rightly flags. By
interface-segregation, add a focused sibling:

```rust
// pinion-runtime/src/command/sink.rs — sibling of IntentSink (runtime-agnostic, §6.3)
pub trait RepaintSink: Send + Sync + 'static {
    fn request_repaint(&self, scope: RepaintScope);   // RepaintScope::All | Window(String)
}
```

```rust
// pinion-shell/src/executor.rs — sibling of ProxyIntentSink; the ONLY winit-aware part
pub struct ProxyRepaintSink { proxy: EventLoopProxy<AppEvent> }
impl RepaintSink for ProxyRepaintSink {
    fn request_repaint(&self, scope: RepaintScope) {
        let _ = self.proxy.send_event(AppEvent::ExternalRepaint(scope.into_window_id()));
    }
}
```

```rust
// pinion-shell/src/lib.rs — additive (AppEvent is already pub)
pub enum AppEvent { /* … */, ExternalRepaint(Option<String>) }   // None = all windows
// handler (app.rs user_event):
AppEvent::ExternalRepaint(scope) => match scope {
    Some(id) => self.core.request_redraw_for_window(&id),
    None     => self.core.request_redraw(),
},   // existing drain_redraw_to_winit tail (app.rs:1722) + winit coalescing finish the job
```

- **Distinct semantic edge**, justified exactly as `IntentArrived` /
  `RpcRequest` / `AccessKit` / `WindowsDirty` are each distinct — NOT an
  overload. `WindowsDirty` keeps its window-topology meaning.
- **Runtime-agnostic**: pinion-runtime defines `RepaintSink`; the shell owns the
  one `EventLoopProxy` impl. Consumers never see winit.
- **Coalescing is free**: many `ExternalRepaint` wakes between frames each set
  the same `redraw_requested` flag → one `request_redraw`; winit collapses
  multiple `request_redraw` into one `RedrawRequested`. A noisy child (`yes`)
  cannot force more than one paint per frame.

### 3.2 Injection: seed the sink into the root Owner (R3.2 — no new mechanism)

The binding obtains the sink the SAME way it obtains `local_task_pump()` /
the `CommandExecutor` — through the root `Owner`, not through `main()`:

```rust
// shared boot path (run / run_with_handlers): seed once, like local_task_pump
core.root_owner().provide_repaint_sink(Arc::new(ProxyRepaintSink { proxy }));
// binding side:
pub fn use_repaint_sink() -> Arc<dyn RepaintSink> { Owner::current()...  }  // sibling of use_local_task_pump
```

No `run_with_external`, no boot value threaded through `main()`. Because the
sink only needs the proxy (no tokio), seed it in the **shared boot path** so
even plain `run` supports external repaint — **zero new entry point**.

### 3.3 Data + producer: reuse `use_X` / `Owner::cache` / R969 (R3.2/R3.3)

No new data-injection mechanism. The grid is shared reactive state the binding
self-creates, exactly the `use_storage` (R665) / `Resource` (R923) pattern:

```rust
// binding: self-created shared handle in an Owner::cache hook (sibling of use_storage)
pub fn use_terminal_grid() -> Rc<TermGridState>  // wraps Arc<Mutex<GridBuffer>> (Send)

fn view(state, frame) -> Scene {
    let grid = use_terminal_grid();
    text_grid_node(&grid.snapshot())   // reads the shared handle, like R923 resource.state()
}
```

- The **writer thread** is a classic blocking `std::thread` PTY reader **owned
  by the binding** (R969 — sprag owns the engine; no tokio needed for a blocking
  `read()`). Spawned in an Effect (R923 `create_extra_externals` install + R665
  retention), capturing a clone of the `Arc<Mutex<GridBuffer>>` + `use_repaint_sink()`.
- **Config** (shell command / cwd / env) is read in the hook factory from env /
  a config struct — the todomvc `PINION_STORAGE_DIR` precedent — so nothing
  flows through `main()`. A typed boot value is lifted only if a 2nd binding
  ever needs it (`abstraction-needs-second-consumer`); the single terminal
  consumer does not.
- **Stream shape (R3.3):** this is the continuous-producer generalization of the
  R923 fetch-shaped async-data substrate. The blessed pattern: *Effect spawns
  reader → reader writes shared buffer → reader calls `request_repaint()` →
  `view` re-reads buffer → fresh `Scene::TextGrid`.* Event-driven, no polling.

### 3.4 `view` reading the shared handle vs a per-frame reactive mirror

- **Recommended (option a):** `view` reads `Arc<Mutex<GridBuffer>>` directly —
  identical in spirit to R923's `view` reading `resource.state()` (async-mutated
  external data). Sync lock, short critical section.
- **Alternative (option b):** a per-frame generation-gated mirror (reusing R978
  per-row `generation` damage) copies changed rows into a reactive `Signal`,
  confining the cross-thread boundary to one place (the `substrate.rs:2558` pump
  drain) so `view` reads only the reactive layer. Lift to (b) **only** if a 2nd
  consumer (e.g. an Effect reacting to grid changes) appears
  (`abstraction-needs-second-consumer`). For pure display, (a) suffices.

## 4. Why this is textbook / SSOT / north-star

- **SSOT:** adds one focused trait *on* the existing §6.3 boundary layer
  (`Executor`/`IntentSink` → `+ RepaintSink`), reusing `Owner::cache`/`use_X`
  for data, R969 for authority, `run_*` boot wiring for injection, winit redraw
  coalescing for debounce. No parallel mechanism.
- **§6.3 / dry_run:** `view` reading the shared grid is no less pure than reading
  a `Signal` or `resource.state()` — both routine. `State: Copy` is **kept**;
  the grid is deliberately not `State`. dry_run determinism is over SCE state
  transitions; the grid is producer-authoritative display data dry_run does not
  predict (R969).
- **§2 invariants:** painted content stays `Scene::TextGrid` (scene-as-data,
  fully introspectable); `WindowsDirty` semantics preserved.
- **North-star:** `RepaintSink` is generic live-async-producer infrastructure —
  any future live-data widget (log tailer, network/process monitor, profiler
  stream) reuses the same edge. It is a real GUI-framework primitive on the path
  to the self-hosted editor, not a terminal one-off.
- **Interface segregation:** a producer that only repaints must not depend on
  `send(Intent)`; two focused sinks beat one fat sink.

## 5. Open questions for ratification

1. **`view` read shape:** option (a) direct shared-handle read (recommended) vs
   (b) per-frame reactive mirror. Recommend (a); lift to (b) on a 2nd consumer.
2. **`RepaintScope`:** `Option<window-id>` (recommended, multi-window-ready) vs
   all-windows-only. Recommend the scoped form.
3. **Sink home:** `RepaintSink` next to `IntentSink` in
   `pinion-runtime/src/command/sink.rs` (recommended) vs a new boundary module.
4. **Seed site:** shared boot path so plain `run` + `run_with_handlers` both
   support it (recommended) vs `run_with_handlers` only.

## A. Rejected design (recorded for the audit trail)

The first draft proposed, and the adversarial audit rejected:

- **`run_with_external` entry point** — parallel boot path; the terminal already
  wants the executor surface (`run_with_handlers`), and the sink seeds in the
  shared boot path, so no new entry point is needed.
- **`use_boot` / typed associated `BootData` / type-erased `Box<dyn Any>`** —
  all three duplicate the existing `use_X` + `Owner::cache` + `Resource`
  data-injection SSOT; the `Box<dyn Any>` downcast is itself an anti-pattern.
  The binding self-creates its shared handle; nothing flows through `main()`.
- **Raw `EventLoopProxy`-backed `WakeHandle` exposed to consumers** — leaks
  winit into consumer code and bypasses the §6.3 boundary-trait SSOT. Replaced
  by the runtime-agnostic `RepaintSink` trait + shell-owned `ProxyRepaintSink`.

Root cause of the bad first draft: reasoning from `AppEvent` + reactive +
`spawn_stdin_rpc_reader` while overlooking the `Executor`/`IntentSink`/`Resource`
async boundary substrate (§1).

## 6. Scope & acceptance criteria

Net new pinion surface: **one trait (`RepaintSink`), one shell impl
(`ProxyRepaintSink`), one `AppEvent` variant (`ExternalRepaint`), one
owner-seed + `use_repaint_sink` hook.** Touches `pinion-runtime` (trait),
`pinion-shell` (impl + variant + seed). Plausibly a single round.

- [ ] Background thread triggers repaint via an owner-obtained `Arc<dyn RepaintSink>`
      — no global static, no fork of `run()`, no raw proxy. → `RepaintSink` + `use_repaint_sink`.
- [ ] `view()` reads externally-owned `Arc<Mutex<…>>` via a `use_X` hook each
      frame. → `use_terminal_grid` (existing `use_storage`/`Resource` pattern).
- [ ] `WindowsDirty` used only for window topology. → dedicated `ExternalRepaint`.
- [ ] Window repaints from PTY output alone, event-driven (no polling). →
      `request_repaint` → `request_redraw`; verified headlessly via
      `PINION_HIDDEN_WINDOW` + a deterministic write→repaint test.

## 7. Companion sprag work (out of scope, reference)

(1) `sprag-host` fills the rect via `pinion_runtime::compute_layout` (projection
SSOT `sprag_grid::project → text_grid_node` unchanged). (2) `sprag-terminal`
reader thread calls `RepaintSink::request_repaint` on each parsed batch.
