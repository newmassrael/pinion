//! R2088 §5.16 — a surface whose `configure` was **refused** must say so,
//! and asking it for an image must skip the frame rather than kill the
//! process.
//!
//! # The defect this is the guard for
//!
//! A full demo sweep failed one walk per run, and a *different* walk each
//! run — always one that detaches a panel into a second window (that
//! population is a command, not a number: `grep -lE "tear_off|undock|detach"
//! tools/demos/*.py`, which answered 29 of 109 when the defect was
//! registered and 47 of 725 at R2088 — do not copy either figure forward).
//! The harness saw an RPC timeout; the log said the renderer had
//! panicked inside `wgpu` with `Surface is not configured for
//! presentation`. So the timeout was the symptom and the cause was a dead
//! process, which is why re-running "fixed" it and why no walk owned it.
//!
//! Measured at R2088, against `wgpu` 29's own source, the chain is:
//!
//! 1. `wgpu::Surface::configure` returns `()`. Its only failure channel is
//!    the device's error sink, so a caller that does not open an error
//!    scope cannot tell a configure that worked from one that did not.
//! 2. `wgpu-core` clears the surface's `presentation` on the way into a
//!    reconfigure and only re-establishes it on success, so a refused
//!    configure leaves the surface *not configured for presentation*.
//! 3. `get_current_texture()` on such a surface is not a status a caller
//!    can classify. When the surface has never been configured
//!    successfully it is reported through `handle_error_fatal`, which
//!    consults **no** uncaptured-error handler and panics the process —
//!    and a surface made by the recovery ladder's heavy rung
//!    (`Rung::Rebuilt`) is exactly a surface that has never been
//!    configured successfully.
//!
//! A freshly created window is what climbs that ladder, which is why the
//! failure was second-window-shaped and why it was rare.
//!
//! # Why this test can be deterministic when the defect was not
//!
//! It does not reproduce the race. It reproduces the **state** the race
//! reaches, by the shortest refusal `wgpu` documents: hold an acquired
//! swapchain image and reconfigure. `wgpu` answers `SurfaceOutput must be
//! dropped before a new Surface is made`, and everything downstream is the
//! same. Run against the pre-R2088 tree this file does not fail — it
//! *aborts*, with the sweep's own panic text.
//!
//! `#[ignore]` like the other GPU tests: it needs an adapter and a real X
//! display, which the windowless `cargo test --workspace` has neither of.
//! CI runs it in the demo-sweep job, on the same Xvfb display and software
//! adapter the sweep itself uses.

use pinion_gpu::{GpuContext, Missed, Rung};
use std::sync::Arc;
use vello::wgpu;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::platform::x11::EventLoopBuilderExtX11;
use winit::window::{Window, WindowId};

/// What the window-side body observed, carried back out of the event loop.
///
/// Collected rather than asserted in place so a failure is reported by the
/// test harness with the whole sequence in view, and so "the body never
/// ran" is distinguishable from "the body ran and agreed" — an event loop
/// that resumes zero times would otherwise pass vacuously.
#[derive(Default)]
struct Observed {
    steps: Vec<String>,
}

impl ApplicationHandler for Observed {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        // ⚠ R2091 — BOTH bodies run inside this one event loop, each on its
        // own window, and that is not a style choice: winit allows exactly one
        // `EventLoop` per PROCESS, so a second `#[test]` building its own dies
        // `RecreationAttempt` — measured, when the device-loss body was first
        // written as a separate test. Two windows, one loop, one verdict.
        self.steps = observe(&Self::window(el));
        self.steps.extend(observe_device_loss(&Self::window(el)));
        el.exit();
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}
}

impl Observed {
    /// A window for a surface to be made against. Never mapped: the surface,
    /// the swapchain and the configure are all real, and no window flashes on
    /// the developer's display — the same bargain `PINION_HIDDEN_WINDOW`
    /// makes for every demo.
    fn window(el: &ActiveEventLoop) -> Arc<Window> {
        Arc::new(
            el.create_window(
                Window::default_attributes()
                    .with_visible(false)
                    .with_inner_size(winit::dpi::LogicalSize::new(320.0, 240.0)),
            )
            .expect("create the window this surface is for"),
        )
    }
}

fn observe(window: &Arc<Window>) -> Vec<String> {
    let mut steps = Vec::new();
    let size = window.inner_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let (context, mut surface) = pollster::block_on(GpuContext::new(
        Arc::clone(window),
        width,
        height,
        wgpu::PresentMode::AutoVsync,
    ))
    .expect("a context for a window this host can present to");

    // A surface handed back by `GpuContext::new` is presentable, and R2088
    // is what makes that a fact rather than an assumption: a first
    // configure that is refused now fails the constructor instead of
    // returning a surface whose first acquisition aborts the process.
    assert!(
        surface.is_presentable(),
        "a freshly built surface must be configured for presentation"
    );
    let held = surface
        .acquire()
        .expect("a healthy surface hands over an image");
    steps.push("acquired an image from a healthy surface".to_owned());

    // The refusal. `wgpu` will not reconfigure a surface whose previous
    // output is still alive, and `held` is alive.
    context.resize_surface(&mut surface, width, height);
    assert!(
        !surface.is_presentable(),
        "a refused configure must leave the surface NOT presentable — this is \
         the fact `wgpu` reports only through an error scope, and reading its \
         silence as success is the whole defect"
    );
    steps.push("a refused reconfigure is reported, not swallowed".to_owned());

    // ★ R2088.1 — and the REASON survives. R2088 replaced the
    // uncaptured-error handler's printout with an error scope, which captures
    // the refusal, while the recovery ladder discards the `Result` — so the
    // one sentence saying why a window is dark had no channel left. It is
    // held on the surface until read.
    let why = surface
        .take_refusal()
        .expect("the refusal is kept for a reader, not only returned to a caller who drops it");
    assert!(
        why.contains("SurfaceOutput"),
        "the kept reason is wgpu's own sentence, not a summary of it: {why}"
    );
    assert_eq!(
        surface.take_refusal(),
        None,
        "taken, so a dark window says it once per refusal and not once per frame"
    );
    steps.push("the reason a configure was refused survives to a reader".to_owned());

    // ★ The assertion the sweep was dying on. Before R2088 this line was
    // `get_current_texture()` on an unconfigured surface, which panics the
    // process — so on the pre-R2088 tree the run ABORTS here rather than
    // reporting a failure.
    assert_eq!(
        surface.acquire().err(),
        Some(Missed::Unconfigured),
        "an unconfigured surface must be refused BEFORE wgpu is asked"
    );
    steps.push("an unconfigured surface refuses instead of aborting".to_owned());

    // And the refusal is a skipped frame, not a dead window: it is an
    // invalidation, so it earns a rung, and the rung puts the surface back.
    //
    // ⚠ Releasing the stranded image is itself a fatal path in `wgpu`, and
    // it is a THIRD one: a `SurfaceTexture` dropped un-presented calls
    // `Surface::discard_texture`, which reports through
    // `handle_error_fatal` — no error sink consulted, so no scope and no
    // handler can catch it. `present()` reports through the sink instead,
    // so it is catchable, and the scope here catches its refusal. That is a
    // fact about this test's setup rather than about pinion: no pinion path
    // holds an image across a reconfigure, because a reconfigure only ever
    // happens between frames.
    let scope = context
        .device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    held.present();
    drop(pollster::block_on(scope.pop()));
    assert_eq!(
        context.recover(&mut surface, Missed::Unconfigured),
        Some(Rung::Reconfigured),
        "the refusal must earn the cheap rung of the recovery ladder"
    );
    assert!(
        surface.is_presentable(),
        "the rung must have restored presentability once the held image was gone"
    );
    let recovered = surface
        .acquire()
        .expect("the window presents again after the ladder's cheap rung");
    drop(recovered);
    steps.push("the ladder puts the window back on the screen".to_owned());
    steps
}

/// R2091 — what a **destroyed** device does, and the attempt this refutes.
///
/// # The attempt, and why it failed
///
/// The debt this file guards has said since R2088.1 that a device loss
/// "cannot be forced on this host", so every layer built for it rests on
/// measured source facts plus an intermittent observation (about one event
/// per eighty second-window lifetimes on this machine). `wgpu::Device::destroy`
/// looked like the missing trigger: it clears `wgpu-core`'s `valid` flag, and
/// the device-lost callback is queued inside `Device::maintain` for exactly
/// `!is_valid() && queue_empty`, which is where a driver-induced loss reaches
/// it too.
///
/// ⛔ **Measured at R2091, it is not.** Destroy the device, run a maintain
/// through an empty submit, and this host's acquisition comes back
/// `Missed::Validation` — the callback has NOT fired, so
/// `DeviceLiveness::is_lost` is still false and the refusal comes from
/// `wgpu`'s status rather than from pinion's own check. So a destroyed device
/// and a lost device are **not** the same fact to this surface, and the debt's
/// sentence stands. ⇒ **Do not spend another round on `destroy` as a
/// substitute for a device loss.**
///
/// # What it is a guard for, then
///
/// The state is still one no test covered: a surface whose device has been
/// destroyed under it. It answers a CLASSIFIED miss rather than aborting, the
/// ladder runs on it, and the window does not come back — which is the shape
/// `debt-a-lost-device-leaves-a-window-dark-for-the-rest-of-the-run` records
/// from the churn observation, asserted here without waiting for one.
fn observe_device_loss(window: &Arc<Window>) -> Vec<String> {
    let mut steps = Vec::new();
    let size = window.inner_size();
    let (width, height) = (size.width.max(1), size.height.max(1));
    let (context, mut surface) = pollster::block_on(GpuContext::new(
        Arc::clone(window),
        width,
        height,
        wgpu::PresentMode::AutoVsync,
    ))
    .expect("a context for a window this host can present to");

    // Present rather than drop: an un-presented `SurfaceTexture` releases
    // through `discard_texture`, which is fatal and catchable by nothing.
    surface
        .acquire()
        .expect("a healthy surface hands over an image")
        .present();
    steps.push("the window is presenting before the device goes".to_owned());

    // THE LOSS, on demand.
    context.device().destroy();
    // Nothing has told the surface yet: the callback is queued by a maintain,
    // and no frame has run one. This is the gap R2089 measured inside the
    // emitted renderer — between a rung and the retry acquire there were zero
    // submits — so it is asserted here rather than assumed.
    assert!(
        surface.is_presentable(),
        "destroying the device does not by itself reach the surface: the fact \
         travels on a callback that only a maintain fires"
    );
    steps.push("a destroyed device has not reached the surface yet".to_owned());

    // ★ The operation R2089 added to `GpuContext::recover`, run here for the
    // same reason: `Queue::submit` calls `maintain(PollType::Poll)`, and that
    // maintain is the only place the device-lost closure is queued. Submitting
    // nothing adds no work and moves the queue toward the `queue_empty` the
    // closure also waits on. Its own failure reports through the sink, so it
    // is not a way to die.
    context.queue().submit(core::iter::empty());

    // ⛔ THE REFUTATION, ASSERTED SO IT CANNOT BE FORGOTTEN. If destroying the
    // device fired the callback, this would be `Missed::DeviceLost` and
    // pinion's own check would have refused before `wgpu` was asked. Measured:
    // it is `Missed::Validation`, from `wgpu`'s status. The two facts are
    // different and this line is what keeps the next round from re-trying the
    // trigger — while still asserting the part that matters: the answer is
    // CLASSIFIED and the process is alive.
    let refused = surface.acquire().err();
    assert_eq!(
        refused,
        Some(Missed::Validation),
        "a destroyed device is refused by wgpu's status, NOT by the device-lost \
         callback — `Device::destroy` does not fire it on this host, which is \
         why it cannot stand in for a device loss"
    );
    assert!(
        surface.acquire().is_err(),
        "and it stays refused rather than intermittently handing over an image"
    );
    steps.push("a destroyed device is a classified miss, not an abort".to_owned());

    // And the ladder answers on it. `Validation` is an invalidation, so it
    // earns a rung — and the rung cannot put this window back, because every
    // rung remakes a surface and none remakes a device. That is the shape
    // `debt-a-lost-device-leaves-a-window-dark-for-the-rest-of-the-run`
    // records from a churn observation; asserting it here means the next
    // reader does not have to wait for one.
    let rung = context.recover(&mut surface, refused.expect("a refusal to recover from"));
    assert!(
        rung.is_some(),
        "an invalidation must earn a rung of the recovery ladder"
    );
    // ⚠⚠ AND HERE IS A MEASUREMENT THIS ROUND DID NOT EXPECT, REPORTED RATHER
    // THAN ASSERTED. The rung's configure is NOT refused on a destroyed
    // device — no error reaches the scopes `GpuSurface::configure` opens — so
    // the surface CLAIMS presentability again while every acquisition still
    // fails. That is R2088's own headline defect surviving for this trigger:
    // a repair that judges by the ABSENCE of an error believes a silence.
    //
    // It is deliberately not asserted in either direction. Asserting it TRUE
    // pins a defect (R2086's class); asserting it FALSE would fail the day
    // someone repairs it, which is the flake R2089 introduced and paid for.
    // The step text carries the observation instead, so a run says which
    // world it saw.
    let claimed = surface.is_presentable();
    // What must hold either way: the window is still not getting an image,
    // and asking still does not end the process.
    assert!(
        surface.acquire().is_err(),
        "the ladder cannot remake a device, so the window must still be refused"
    );
    steps.push(format!(
        "the ladder runs and does not abort; presentable={claimed} afterwards \
         (a configure on a destroyed device reaches no error sink)"
    ));
    steps
}

#[test]
#[ignore = "needs a wgpu adapter and a real X display; CI runs it in the demo-sweep job"]
fn a_refused_configure_is_reported_and_survivable() {
    let event_loop = EventLoop::builder()
        .with_x11()
        // A `#[test]` body does not run on the process's main thread, and
        // winit refuses an event loop off it unless asked. X11 only, which
        // is what the sweep's Xvfb display is.
        .with_any_thread(true)
        .build()
        .expect("an X11 event loop on this display");
    let mut observed = Observed::default();
    event_loop
        .run_app(&mut observed)
        .expect("run the event loop");
    // R2091 — say what was observed rather than only how many. One of these
    // steps carries a MEASUREMENT (whether a destroyed device's surface still
    // claims presentability), and a number cannot report it.
    for (n, step) in observed.steps.iter().enumerate() {
        println!("[gpu-guard {}] {step}", n + 1);
    }
    assert_eq!(
        observed.steps.len(),
        9,
        "both window bodies must have run to the end; observed {:?}",
        observed.steps
    );
}
