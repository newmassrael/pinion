//! R1570 §5.16 — a `#[widget]` forward names an INHERENT function, and the
//! compiler is what says so.
//!
//! The `#[widget(...)]` attribute macro generates trait impls whose bodies
//! forward to an inherent function of the same name on the view type
//! (`fn keybinding(key) { <TheView>::keybinding(key) }`). That call resolves
//! to an inherent associated function when one exists — and, when one does
//! **not**, silently to the trait method it is standing inside. The forward
//! then calls itself, unconditionally, forever.
//!
//! Nothing catches that on its own. `unconditional_recursion` is warn-by-default
//! and does not fire for a trait-qualified associated-function path (measured at
//! R1570: `#[deny(unconditional_recursion)]` on the generated body compiles
//! silently). In debug the runaway would at least blow the stack; in release the
//! self-call is a tail call, so it becomes a bare `jmp` — a 100% CPU loop that
//! allocates nothing, makes no syscall, and never returns. The process stays
//! alive and answers nothing.
//!
//! That is not hypothetical. R1570 found four shipped bindings in exactly this
//! state (`hello-richtext-background` / `-blocks` / `-list` / `-cells`, each
//! declaring `keybinding` in its flag list with no inherent function to match),
//! latent from the round that wrote them until R1569 gave `WidgetCore::keybinding`
//! a caller on the RPC read path. One character key had always been enough.
//!
//! # The mechanism
//!
//! Each forwarded name gets a guard trait here, blanket-implemented for every
//! type. The macro brings the matching guard into scope **inside the generated
//! body only**, so at the forward's call site there are two applicable
//! candidates whenever the inherent function is missing, and rustc rejects it
//! as [E0034] naming the guard. When the inherent function *is* present it wins
//! outright — an inherent associated function shadows every trait candidate —
//! so a correct binding compiles unchanged and pays nothing.
//!
//! The guards' own signatures are deliberately nullary and unrelated to the
//! forwarded ones: ambiguity is decided by NAME, before any signature is
//! checked, so a guard does not have to know the view's `State` or `Event`
//! types to discriminate. This is what lets one guard cover every binding.
//!
//! [E0034]: https://doc.rust-lang.org/error_codes/E0034.html
//!
//! Nothing here is meant to be called, named, or implemented by hand — the
//! macro imports each guard anonymously (`as _`). They are `pub` only because
//! the code that imports them is generated into other crates.

/// R1570 §5.16 — declares the guard trait for one `#[widget]` forward.
///
/// One trait per forwarded name, because ambiguity is per name: bundling them
/// would bring every guarded name into scope at every forward, widening the
/// blast radius of a mechanism whose whole value is that it is narrow.
macro_rules! forward_guard {
    ($(#[$meta:meta])* $trait_name:ident, $method:ident) => {
        $(#[$meta])*
        pub trait $trait_name {
            /// Never called. Exists to be a second candidate for the
            /// forwarded name when the view declares no inherent function.
            fn $method() {}
        }

        impl<T: ?Sized> $trait_name for T {}
    };
}

forward_guard!(
    /// The view type declares no inherent `read_state`, so `#[widget]`'s
    /// forward would call the trait method it is defining. Write
    /// `fn read_state(scene: &Scene) -> State` on the view, or let the macro
    /// derive it.
    ReadStateNeedsInherentFn,
    read_state
);

forward_guard!(
    /// The view type declares no inherent `event_name`, so `#[widget]`'s
    /// forward would call the trait method it is defining. Write
    /// `fn event_name(event: Event) -> &'static str` on the view, or let the
    /// macro derive it.
    EventNameNeedsInherentFn,
    event_name
);

forward_guard!(
    /// The view type declares no inherent `view`, so `#[widget]`'s forward
    /// would call the trait method it is defining. Write
    /// `fn view(state: State, frame: Frame) -> Scene` on the view.
    ViewNeedsInherentFn,
    view
);

forward_guard!(
    /// `#[widget(initial_size_strategy)]` was declared and the view type has
    /// no inherent `initial_size_strategy`. Write one, or drop the flag and
    /// let `initial_size = (w, h)` stand.
    InitialSizeStrategyNeedsInherentFn,
    initial_size_strategy
);

forward_guard!(
    /// `#[widget(apply_key)]` was declared and the view type has no inherent
    /// `apply_key`. Write one, or drop the flag.
    ApplyKeyNeedsInherentFn,
    apply_key
);

forward_guard!(
    /// `#[widget(keybinding)]` was declared and the view type has no inherent
    /// `keybinding`. Write one, or drop the flag.
    KeybindingNeedsInherentFn,
    keybinding
);

forward_guard!(
    /// `#[widget(fmt_state_log)]` was declared and the view type has no
    /// inherent `fmt_state_log`. Write one, or drop the flag.
    FmtStateLogNeedsInherentFn,
    fmt_state_log
);

forward_guard!(
    /// `#[widget(update)]` was declared and the view type has no inherent
    /// `update`. Write one, or drop the flag.
    UpdateNeedsInherentFn,
    update
);

forward_guard!(
    /// The view type declares no inherent `access_node` and `#[widget]` was
    /// given no `role`, so the a11y forward would call the trait method it is
    /// defining. Write one, or declare `role = ...`.
    AccessNodeNeedsInherentFn,
    access_node
);

#[cfg(test)]
mod tests {
    //! The guards are compile-time, so what can be asserted at runtime is that
    //! they discriminate the way the macro relies on: an inherent function of
    //! the same name shadows the blanket impl. A view that *does* declare one
    //! must keep reaching its own code — the negative control for the whole
    //! mechanism, since a guard that also broke correct bindings would be
    //! caught by the workspace failing to build but not by anything explaining
    //! why.

    use super::{KeybindingNeedsInherentFn, ViewNeedsInherentFn};

    struct Declared;

    impl Declared {
        fn keybinding(key: &str) -> Option<u8> {
            key.bytes().next()
        }

        fn view(depth: u8) -> u8 {
            depth + 1
        }
    }

    #[test]
    fn an_inherent_fn_shadows_the_guard() {
        #[allow(unused_imports)]
        use super::KeybindingNeedsInherentFn as _;

        // Resolves to `Declared::keybinding`, not to the guard's nullary
        // method — which is the property the generated forward rests on.
        assert_eq!(<Declared>::keybinding("q"), Some(b'q'));
    }

    #[test]
    fn each_guard_is_a_distinct_name() {
        #[allow(unused_imports)]
        use super::{KeybindingNeedsInherentFn as _, ViewNeedsInherentFn as _};

        // Both guards in scope at once and neither shadows the other's
        // forwarded name: one trait per name is what keeps a `view` forward
        // from being decided by a `keybinding` guard.
        assert_eq!(<Declared>::view(1), 2);
        assert_eq!(<Declared>::keybinding(""), None);
    }

    #[test]
    fn the_guard_is_implemented_for_every_type() {
        // The blanket impl is what makes the mechanism uniform: the macro
        // cannot know which types it will be expanded for, so the guard has to
        // apply to all of them.
        fn assert_guarded<T: KeybindingNeedsInherentFn + ViewNeedsInherentFn + ?Sized>() {}

        assert_guarded::<Declared>();
        assert_guarded::<str>();
        assert_guarded::<[u8]>();
    }
}
