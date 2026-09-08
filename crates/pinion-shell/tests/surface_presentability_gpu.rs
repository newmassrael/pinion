//! R2088 §5.16 — a surface whose `configure` was **refused** must say so,
//! and asking it for an image must skip the frame rather than kill the
//! process.
//!
//! # The defect this is the guard for
//!
//! A full demo sweep failed one walk per run, and a *different* walk each
//! run — always one of the twenty-nine that detach a panel into a second
//! window. The harness saw an RPC timeout; the log said the renderer had
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
//! swapchain image and reconfigure. `wgpu` answers "SurfaceOutput must be
//! dropped before a new Surface is made", and everything downstream is the
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
        let window = Arc::new(
            el.create_window(
                Window::default_attributes()
                    // Never mapped: the surface, the swapchain and the
                    // configure are all real, and no window flashes on the
                    // developer's display. The same bargain
                    // `PINION_HIDDEN_WINDOW` makes for every demo.
                    .with_visible(false)
                    .with_inner_size(winit::dpi::LogicalSize::new(320.0, 240.0)),
            )
            .expect("create the window this surface is for"),
        );
        self.steps = observe(&window);
        el.exit();
    }

    fn window_event(&mut self, _el: &ActiveEventLoop, _id: WindowId, _event: WindowEvent) {}
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
    assert_eq!(
        observed.steps.len(),
        4,
        "the window body must have run to the end; observed {:?}",
        observed.steps
    );
}
