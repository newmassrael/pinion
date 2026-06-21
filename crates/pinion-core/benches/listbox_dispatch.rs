//! R51.105 §5.38 — micro-bench for `ListBox` dispatch alloc cost.
//!
//! Measures the per-user-input cost of the R51.102 substrate refactor
//! (`Snapshot = Vec<bool>` + `detect → Vec<Intent>`). The Vec heap
//! allocation lives on the hot path between `IntentEmitter::dispatch`
//! and the user's next event; if the alloc overhead is significant
//! compared to the rest of the dispatch (state-machine transition +
//! bitmap copy + intent push), switching the snapshot to
//! `SmallVec<[bool; N_MAX]>` for stack-on-common-case is the
//! textbook follow-up (R51.105.x carry).
//!
//! Scenarios cover the N-axis (small / medium / large lists) and the
//! mode axis (single vs multi). The dispatch path is identical between
//! modes — only the detect-filter rule changes — so any divergence in
//! observed timing points at the alloc / iteration cost.
//!
//! ## Scenarios
//!
//! 1. `single_n4_activate` — N=4, single-select, one full activate
//!    cycle (`Enter / Down / Up / Leave`). Mirrors hello-listbox.
//! 2. `multi_n4_toggle` — N=4, multi-select, same activate cycle.
//! 3. `multi_n6_toggle` — N=6, multi-select, mirrors hello-listbox-
//!    multi.
//! 4. `multi_n20_toggle` — N=20, multi-select. Stresses the bitmap
//!    `Vec<bool>` clone path at a list size beyond the demos.
//! 5. `single_n4_idle_event` — N=4, single-select, drives a
//!    no-activate event (`PointerEnter` only). Establishes the
//!    "snapshot + drive + empty detect" cost floor.
//!
//! The benches address `ListBoxExternal` (full §5.15 wire) so the
//! `IntentEmitter::dispatch` pipeline is measured end-to-end, not
//! just the raw `ListBox::send`.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use pinion_core::external::External;
use pinion_core::widgets::listbox::ListBoxExternal;
use pinion_core::widgets::listbox_item::ListboxItemEvent;

fn drain(lb: &mut ListBoxExternal) {
    lb.drain_intents(&mut |_| {});
}

fn full_activate(lb: &mut ListBoxExternal, idx: usize) {
    for ev in [
        ListboxItemEvent::PointerEnter,
        ListboxItemEvent::PointerDown,
        ListboxItemEvent::PointerUp,
        ListboxItemEvent::PointerLeave,
    ] {
        lb.send(idx, ev);
    }
}

fn single_n4_activate(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_n4_activate");
    group.bench_function(BenchmarkId::from_parameter("dispatch"), |b| {
        b.iter_with_setup(
            || ListBoxExternal::new(4),
            |mut lb| {
                full_activate(black_box(&mut lb), 1);
                drain(&mut lb);
            },
        );
    });
    group.finish();
}

fn multi_n4_toggle(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_n4_toggle");
    group.bench_function(BenchmarkId::from_parameter("dispatch"), |b| {
        b.iter_with_setup(
            || ListBoxExternal::with_multiselect(4),
            |mut lb| {
                full_activate(black_box(&mut lb), 1);
                drain(&mut lb);
            },
        );
    });
    group.finish();
}

fn multi_n6_toggle(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_n6_toggle");
    group.bench_function(BenchmarkId::from_parameter("dispatch"), |b| {
        b.iter_with_setup(
            || ListBoxExternal::with_multiselect(6),
            |mut lb| {
                full_activate(black_box(&mut lb), 3);
                drain(&mut lb);
            },
        );
    });
    group.finish();
}

fn multi_n20_toggle(c: &mut Criterion) {
    let mut group = c.benchmark_group("multi_n20_toggle");
    group.bench_function(BenchmarkId::from_parameter("dispatch"), |b| {
        b.iter_with_setup(
            || ListBoxExternal::with_multiselect(20),
            |mut lb| {
                full_activate(black_box(&mut lb), 10);
                drain(&mut lb);
            },
        );
    });
    group.finish();
}

fn single_n4_idle_event(c: &mut Criterion) {
    // PointerEnter from Idle → no activation → empty detect. Floor
    // for the snapshot + drive + detect overhead without the intent
    // push cost.
    let mut group = c.benchmark_group("single_n4_idle_event");
    group.bench_function(BenchmarkId::from_parameter("dispatch"), |b| {
        b.iter_with_setup(
            || ListBoxExternal::new(4),
            |mut lb| {
                lb.send(black_box(0), ListboxItemEvent::PointerEnter);
            },
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    single_n4_activate,
    multi_n4_toggle,
    multi_n6_toggle,
    multi_n20_toggle,
    single_n4_idle_event,
);
criterion_main!(benches);
