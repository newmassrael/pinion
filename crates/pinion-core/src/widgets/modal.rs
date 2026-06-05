//! R788 §5.39 §5.16 — the modal **open-lifecycle** reactive holder: the
//! lifted SSOT for "is this modal up, and is its focus trap installed".
//!
//! ## Why this exists (the 3rd-consumer lift)
//!
//! [`hello-dialog`](../../../../examples/hello-dialog) (R693) and
//! [`hello-drawer`](../../../../examples/hello-drawer) (R702) each grew the
//! *same* two-line coupling by hand: a `Signal<bool>` "open" flag plus the
//! [`crate::modal_scope_request`] focus-trap lifecycle, flipped together —
//!
//! ```text
//! open : open_flag.set(true)  ; modal_scope_request::open(members)
//! close: open_flag.set(false) ; modal_scope_request::close()
//! ```
//!
//! That pairing is **correctness-critical, not incidental**: flip the flag
//! without moving the scope (or vice versa) and the painted modal desyncs
//! from the focus trap — the background becomes Tab-reachable behind a
//! visible scrim, or a dismissed dialog leaves Tab trapped on nothing
//! (`divergence-is-a-bug`). When R788's modal file-open dialog arrived as
//! the **third** consumer, that mechanical 3-copy crossed the
//! `[[three-site-internal-duplication-substrate-lift]]` threshold, so the
//! coupling lives here once. The per-binding *outcome* state (the dialog's
//! accepted/cancelled result, the drawer's active destination, the file
//! picker's chosen path) stays in each binding — that is policy, not the
//! mechanism, and it is where the three genuinely diverge.
//!
//! ## What is *not* here
//!
//! - The focus trap's auto-focus / Tab-confinement / restore-on-close: that
//!   is `pinion_runtime::FocusManager`, driven by the
//!   [`crate::modal_scope_request`] the shell drains. [`ModalState`] only
//!   *requests* it in lockstep with the open flag.
//! - The modal's *members* (its focusable controls in Tab order): passed at
//!   [`ModalState::open`] time, since they are the binding's tags.
//! - The scrim / panel chrome: `pinion_widget_paint::dialog` /
//!   `::drawer`.

use std::rc::Rc;

use crate::event::Event;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};
use crate::reactive::{Owner, Signal};
use crate::widget_core::ExtraExternal;

/// R788 — the open-lifecycle of one modal surface (dialog / drawer / file
/// picker). Holds the reactive `open` flag; [`open`](Self::open) /
/// [`close`](Self::close) move it **and** the [`crate::modal_scope_request`]
/// focus-trap lifecycle together, so the two can never drift apart.
///
/// Resolve the shared instance with [`use_modal`] (one `Rc` across the
/// view-fn that reads [`is_open`](Self::is_open), the reducer that calls
/// [`open`](Self::open) / [`close`](Self::close), and `access_node`).
#[derive(Debug)]
pub struct ModalState {
    open: Signal<bool>,
}

impl ModalState {
    /// A fresh, closed modal.
    #[must_use]
    pub fn new() -> Self {
        Self { open: Signal::new(false) }
    }

    /// Whether the modal is currently up. Subscribes when read inside a
    /// view-fn / `access_node` (so opening or closing repaints).
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.open.get()
    }

    /// Open the modal: raise the flag **and** install a focus trap over
    /// `members` (the modal's focusable controls in Tab order — the shell
    /// auto-focuses the first and confines Tab to them). Call from a
    /// reducer / `External::invoke` body, where the request drains in the
    /// same `handle_tail` so the trap is up before the next paint.
    pub fn open(&self, members: Vec<String>) {
        self.open.set(true);
        crate::modal_scope_request::open(members);
    }

    /// Close the modal: lower the flag **and** pop the focus trap
    /// (restoring focus to the invoker). The mirror of [`open`](Self::open);
    /// the per-binding outcome (accepted / cancelled / chosen) is the
    /// caller's to record alongside.
    pub fn close(&self) {
        self.open.set(false);
        crate::modal_scope_request::close();
    }
}

impl Default for ModalState {
    fn default() -> Self {
        Self::new()
    }
}

/// R795 §5.16 §5.12 — the **query-only** RPC introspection face of a
/// [`ModalState`]. Registered as an extra-external (always present in the
/// scene, open *or* closed), it surfaces the open flag at the introspect
/// path `open`, so an AI agent driving over the §5.12 RPC plane can ask
/// "is this modal up?" directly — the same first-class `open` query that
/// [`ContextMenu`](super::context_menu::ContextMenu) already exposes.
/// Before R795 the four modal surfaces (dialog / drawer / file-open /
/// file-save) carried no introspect external, so their open state was only
/// inferable from scene-tree panel presence; this closes that asymmetry
/// without a second source of truth — the flag still lives once in
/// [`ModalState`], and this node merely reads it.
///
/// ## Query-only by construction
///
/// The `open` flag must move in lockstep with the
/// [`crate::modal_scope_request`] focus trap — that lockstep is the entire
/// reason [`ModalState`] exists. A raw `intervene` write would raise the
/// flag *without* installing the trap (the background goes Tab-reachable
/// behind a visible scrim — the exact desync [`ModalState`] prevents), so
/// the `open` slot is read-only: writes are refused with
/// [`InterveneError::ReadOnly`]. Opening and closing go through
/// [`ModalState::open`] / [`ModalState::close`] (a reducer / `External`
/// `invoke` body), never by rewinding this node.
#[derive(Debug)]
pub struct ModalIntrospect {
    state: Rc<ModalState>,
}

impl ModalIntrospect {
    /// Wrap a shared [`ModalState`] as its query-only introspection node.
    /// The `Rc` is cloned, so the view-fn and reducer keep driving the same
    /// open flag this node reports.
    #[must_use]
    pub fn new(state: Rc<ModalState>) -> Self {
        Self { state }
    }
}

impl External for ModalIntrospect {
    /// RPC-only: the node carries no pixels, so the visual backends skip it
    /// (an empty placeholder) while §5.12 `query` still routes through it.
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for ModalIntrospect {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[("open", "bool")])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        if path == "open" {
            Some(IntrospectValue::Bool(self.state.is_open()))
        } else {
            None
        }
    }

    /// The `open` slot is read-only — see the type docs. Opening or closing
    /// must preserve the focus-trap lockstep, so it routes through
    /// [`ModalState::open`] / [`ModalState::close`], not a rewind here.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        if path == "open" {
            Err(InterveneError::ReadOnly)
        } else {
            Err(InterveneError::UnknownPath)
        }
    }
}

/// R788 — resolve the shared [`ModalState`] for `key`, building it once
/// (closed). Mirrors [`use_directory_state`](crate::widgets::file_browser::use_directory_state)
/// / [`use_column_widths`](crate::widgets::column_widths::use_column_widths):
/// the view, the reducer, and `access_node` all call this with the same
/// `key` and receive the same `Rc`, so the open flag is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set — call from within a view /
/// reducer / `create_extra_externals` hook (all run inside a
/// `root_owner.run`).
#[must_use]
pub fn use_modal(key: &'static str) -> Rc<ModalState> {
    Owner::current()
        .expect("use_modal requires an active Owner scope")
        .cache(key, ModalState::new)
}

/// R797 §5.16 §5.12 — register a [`ModalState`]'s query-only introspection
/// face as an [`ExtraExternal`] under `tag`.
///
/// Every modal binding (dialog / drawer / file-open / file-save) wires the
/// *same* `ExtraExternal::new(tag, Box::new(ModalIntrospect::new(state)))`
/// triple in its [`create_extra_externals`](crate::WidgetCore::create_extra_externals)
/// — the boxing + [`ModalIntrospect`] wrapping is mechanical glue the
/// binding must not get wrong (a bare `ButtonExternal` here would silently
/// drop the `open` query). When R797 found that 4-copy past the
/// [[three-site-internal-duplication-substrate-lift]] threshold, the glue
/// moved here once; the binding passes only its `tag` and the shared
/// [`ModalState`] `Rc` (resolved from the same [`use_modal`] key its view
/// and reducer use, so the node reports the live open flag).
#[must_use]
pub fn modal_introspection_extra(tag: &'static str, state: Rc<ModalState>) -> ExtraExternal {
    ExtraExternal::new(tag, Box::new(ModalIntrospect::new(state)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modal_scope_request::{self, ModalRequest};

    #[test]
    fn r788_starts_closed() {
        Owner::new().run(|| {
            assert!(!use_modal("m").is_open(), "a fresh modal is closed");
        });
    }

    #[test]
    fn r788_open_raises_flag_and_requests_trap() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let m = use_modal("m");
            m.open(vec!["ok".to_string(), "cancel".to_string()]);
            assert!(m.is_open(), "open raises the flag");
            assert_eq!(
                modal_scope_request::drain(),
                Some(ModalRequest::Open {
                    members: vec!["ok".to_string(), "cancel".to_string()],
                }),
                "open installs the focus trap over the members in lockstep",
            );
        });
    }

    #[test]
    fn r788_close_lowers_flag_and_pops_trap() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let m = use_modal("m");
            m.open(vec!["ok".to_string()]);
            let _ = modal_scope_request::drain();
            m.close();
            assert!(!m.is_open(), "close lowers the flag");
            assert_eq!(
                modal_scope_request::drain(),
                Some(ModalRequest::Close),
                "close pops the focus trap in lockstep",
            );
        });
    }

    #[test]
    fn r788_same_key_shares_one_flag() {
        Owner::new().run(|| {
            let a = use_modal("shared");
            let b = use_modal("shared");
            a.open(vec!["x".to_string()]);
            assert!(b.is_open(), "the second handle observes the first's open");
        });
    }

    #[test]
    fn r795_introspect_open_tracks_the_shared_flag() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let m = use_modal("m");
            let node = ModalIntrospect::new(m.clone());

            assert_eq!(
                node.query("open"),
                Some(IntrospectValue::Bool(false)),
                "a fresh modal introspects as closed",
            );

            m.open(vec!["ok".to_string()]);
            assert_eq!(
                node.query("open"),
                Some(IntrospectValue::Bool(true)),
                "open() flips the introspected flag (shared SSOT, no second copy)",
            );

            m.close();
            assert_eq!(
                node.query("open"),
                Some(IntrospectValue::Bool(false)),
                "close() lowers the introspected flag",
            );
        });
    }

    #[test]
    fn r797_introspection_extra_carries_tag_and_shared_flag() {
        Owner::new().run(|| {
            let _ = modal_scope_request::drain();
            let m = use_modal("m");
            let extra = modal_introspection_extra("dialog_state", m.clone());
            assert_eq!(extra.tag, "dialog_state", "the helper registers under the given tag");
            let introspect = extra
                .handle
                .introspect()
                .expect("the modal node exposes a query-only introspection face");
            assert_eq!(
                introspect.query("open"),
                Some(IntrospectValue::Bool(false)),
                "the registered node reports the shared closed flag",
            );
            m.open(vec!["ok".to_string()]);
            assert_eq!(
                introspect.query("open"),
                Some(IntrospectValue::Bool(true)),
                "opening the shared ModalState flips the registered node (one SSOT)",
            );
        });
    }

    #[test]
    fn r795_introspect_unknown_path_is_none() {
        Owner::new().run(|| {
            let node = ModalIntrospect::new(use_modal("m"));
            assert_eq!(node.query("members"), None, "only `open` is a query slot");
        });
    }

    #[test]
    fn r795_introspect_open_is_read_only() {
        Owner::new().run(|| {
            let mut node = ModalIntrospect::new(use_modal("m"));
            assert_eq!(
                node.intervene("open", IntrospectValue::Bool(true)),
                Err(InterveneError::ReadOnly),
                "writing `open` is refused: it would bypass the focus-trap lockstep",
            );
            assert_eq!(
                node.intervene("nope", IntrospectValue::Bool(true)),
                Err(InterveneError::UnknownPath),
                "an unknown slot is UnknownPath, distinct from the read-only `open`",
            );
        });
    }

    #[test]
    fn r795_introspect_schema_declares_open_bool() {
        Owner::new().run(|| {
            let node = ModalIntrospect::new(use_modal("m"));
            assert_eq!(
                node.schema().fields,
                &[("open", "bool")],
                "the schema advertises the single read-only `open` bool slot",
            );
        });
    }
}
