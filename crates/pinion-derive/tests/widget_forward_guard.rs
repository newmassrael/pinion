//! R1570 §5.16 — the guard's trait name and its method name agree.
//!
//! [`widget.rs`'s own census] pairs every `<TheView>::name(..)` forward the
//! macro emits with the `widget_forward` guard it imports, and that pairing is
//! by TRAIT NAME. It cannot see one thing: whether the trait so named actually
//! declares a method called `name`. A guard whose method were misspelled would
//! never become a resolution candidate, so the gate would be dead while every
//! test still passed — the failure mode this project keeps finding, where a
//! mechanism is present, asserted, and reaching nothing.
//!
//! The two names live in different crates (the macro emits the import, the
//! trait is declared in `pinion-core`), so nothing but a reference from a place
//! that can see both closes it. Each line below is that reference: it names the
//! trait AND the method, so a rename on either side stops this file compiling.
//!
//! Failure mode is therefore a build failure of `cargo test -p pinion-derive`
//! rather than a red assertion — stated plainly because a compile-time check
//! recorded as a runtime one is exactly the kind of claim that is not worth
//! what it reads as. What it buys is that the mismatch cannot ship.
//!
//! [`widget.rs`'s own census]: ../src/widget.rs

use pinion_core::widget_forward::{
    AccessNodeNeedsInherentFn, ApplyKeyNeedsInherentFn, EventNameNeedsInherentFn,
    FmtStateLogNeedsInherentFn, InitialSizeStrategyNeedsInherentFn, KeybindingNeedsInherentFn,
    ReadStateNeedsInherentFn, UpdateNeedsInherentFn, ViewNeedsInherentFn,
};

/// One entry per guard the macro can import, resolved as a function ITEM so the
/// method name is part of the path being checked.
///
/// `()` is the carrier because it has no inherent associated functions of any
/// of these names — the blanket impl is the only candidate, which is also a
/// live check that the impl really is blanket.
const CORRESPONDENCE: [fn(); 9] = [
    <() as AccessNodeNeedsInherentFn>::access_node,
    <() as ApplyKeyNeedsInherentFn>::apply_key,
    <() as EventNameNeedsInherentFn>::event_name,
    <() as FmtStateLogNeedsInherentFn>::fmt_state_log,
    <() as InitialSizeStrategyNeedsInherentFn>::initial_size_strategy,
    <() as KeybindingNeedsInherentFn>::keybinding,
    <() as ReadStateNeedsInherentFn>::read_state,
    <() as UpdateNeedsInherentFn>::update,
    <() as ViewNeedsInherentFn>::view,
];

#[test]
fn every_guard_declares_the_method_its_name_promises() {
    // Reaching this line at all is the assertion — see the module docs. The
    // count is restated so that DELETING an entry, which would make the file
    // compile while covering less, fails too.
    assert_eq!(CORRESPONDENCE.len(), 9);
}

#[test]
fn a_guard_method_is_inert() {
    // The guards exist to be *candidates*, never to run. If one ever acquired a
    // body with an effect, a binding that forgot its inherent function could go
    // from "does not compile" to "compiles and does something plausible", which
    // is strictly worse than the defect this round closed.
    for entry in CORRESPONDENCE {
        entry();
    }
}
