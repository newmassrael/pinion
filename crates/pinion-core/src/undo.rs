//! R748 §5.52 — **undo / redo command stack** (the `QUndoStack` /
//! `QUndoCommand` peer).
//!
//! A reversible-edit history is the substrate every editor sits on: a
//! linear stack of applied [`UndoCommand`]s plus a cursor splitting the
//! *done* prefix from the *undone* suffix. `undo` moves the cursor back
//! (replaying the inverse of the command it steps over); `redo` moves it
//! forward (re-applying). Pushing a new command truncates the undone
//! suffix — the textbook single-branch model of Qt's `QUndoStack`, Cocoa's
//! `NSUndoManager`, and every word processor's Ctrl+Z.
//!
//! ## Shape (one reactive source of truth)
//!
//! - [`UndoCommand`] — the object-safe reversible unit ([`label`](UndoCommand::label)
//!   / [`redo`](UndoCommand::redo) / [`undo`](UndoCommand::undo)). `label`
//!   makes the history **queryable as data** (§2 #7): an AI agent or an
//!   undo-history panel reads "Increment", "Sort ascending", … without
//!   touching pixels.
//! - [`SignalEdit`] — the common concrete command: snapshot a reactive
//!   [`Signal`]'s value `before`/`after`, restore it on `undo`/`redo`. One
//!   generic helper covers every "a single reactive value changed" edit.
//! - [`UndoStack`] — the stack itself, shared by `Rc` between the reducer
//!   that records edits, the view that greys out a disabled Undo button,
//!   and the [`UndoStackExternal`] that surfaces the history to RPC. Its
//!   observable surface ([`can_undo`](UndoStack::can_undo) /
//!   [`can_redo`](UndoStack::can_redo) / [`index`](UndoStack::index) /
//!   [`labels`](UndoStack::labels)) is **reactive**: every mutation bumps a
//!   monotonic revision [`Signal`], so a view that reads `can_undo` repaints
//!   the moment the stack changes — the exact `ScrollState` /
//!   [`ViewOrderState`](crate::widgets::view_order) sharing pattern this
//!   crate already uses ([`use_undo_stack`]).
//!
//! ## Why a trait, not closures
//!
//! Storing `(Box<dyn Fn()>, Box<dyn Fn()>)` pairs would erase the **label**
//! the AI-first introspection contract needs, and heterogeneous edits (an
//! `i64` `SignalEdit` interleaved with a structural multi-field edit in one
//! Ctrl+Z timeline) already force trait-object erasure — so the
//! `QUndoCommand` trait shape is both the minimal and the canonical choice.
//! The second consumer ([`SortFilterEdit`](crate::widgets::view_order) —
//! a compound `(sort, filter)` edit that is *not* a [`SignalEdit`]) proves
//! the trait earns its erasure.
//!
//! ## Scope (honest boundaries)
//!
//! - **Single linear branch.** Command merging / coalescing (typing
//!   collapses into one undo step) and compound *macro* transactions
//!   (group N edits as one) are additive — both are backward-compatible
//!   later (a defaulted `merge` trait method, a `begin_macro`/`end_macro`
//!   pair) and have no consumer yet ([[abstraction-needs-second-consumer]]).
//! - **Optional capacity.** A bounded stack drops the oldest command from
//!   the front when full (the `QUndoStack::setUndoLimit` model); the
//!   [`use_undo_stack`] hook builds an unbounded stack.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use crate::reactive::{Owner, Signal};

/// A single reversible edit — the object-safe `QUndoCommand` peer.
///
/// [`redo`](Self::redo) applies the edit forward (it is also what
/// [`UndoStack::push`] calls to *first* apply a freshly recorded command,
/// matching `QUndoStack`); [`undo`](Self::undo) applies its inverse. Both
/// must be idempotent with respect to repeated `undo`/`redo` cycling — the
/// canonical implementation captures the `before`/`after` snapshots at
/// record time and restores them verbatim (see [`SignalEdit`]).
pub trait UndoCommand {
    /// Human-/agent-readable description of the edit ("Increment", "Sort
    /// ascending"). Surfaced as data through [`UndoStack::labels`] and the
    /// [`UndoStackExternal`] so the history is introspectable without pixels.
    fn label(&self) -> Cow<'static, str>;

    /// Apply the edit forward (initial application and every redo).
    fn redo(&self);

    /// Apply the edit's inverse.
    fn undo(&self);
}

/// The common concrete [`UndoCommand`]: a reversible write to one reactive
/// [`Signal`], snapshotting its `before`/`after` value.
///
/// Restoring through [`Signal::set`] repaints every view subscribed to the
/// signal exactly as the original edit did — the undo is indistinguishable
/// from the user re-making (or un-making) the change.
pub struct SignalEdit<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    signal: Signal<T>,
    before: T,
    after: T,
    label: Cow<'static, str>,
}

impl<T> SignalEdit<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    /// Record an edit that moves `signal` to `after`, capturing the current
    /// value as the `before` snapshot. The edit is **not** applied here —
    /// [`UndoStack::record`] applies it (via [`redo`](UndoCommand::redo)),
    /// so the stack is the single mutation path (`QUndoStack::push`
    /// semantics).
    #[must_use]
    pub fn to(signal: &Signal<T>, after: T, label: impl Into<Cow<'static, str>>) -> Self {
        Self {
            signal: signal.clone(),
            before: signal.get(),
            after,
            label: label.into(),
        }
    }
}

impl<T> UndoCommand for SignalEdit<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + 'static,
{
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        self.signal.set(self.after.clone());
    }

    fn undo(&self) {
        self.signal.set(self.before.clone());
    }
}

/// A linear undo / redo history of [`UndoCommand`]s with a cursor — the
/// `QUndoStack` peer. Shared by `Rc` (see [`use_undo_stack`]); all methods
/// take `&self` (interior mutability) so the reducer, the view, and the
/// [`UndoStackExternal`] drive the same instance.
pub struct UndoStack {
    /// The applied + undone commands. `commands[..index]` are applied (the
    /// *done* prefix); `commands[index..]` have been undone and can be
    /// redone (the *undone* suffix, truncated on the next [`record`](Self::record)).
    commands: RefCell<Vec<Box<dyn UndoCommand>>>,
    /// Cursor: count of applied commands.
    index: Cell<usize>,
    /// Optional bound; when `Some(n)`, the oldest command is dropped from
    /// the front once the stack exceeds `n`.
    capacity: Option<usize>,
    /// Monotonic mutation counter. Read by the reactive accessors so a view
    /// auto-subscribes and repaints on every stack change; bumped (never
    /// equality-skipped) by every mutating method.
    revision: Signal<u64>,
    /// The [`use_undo_stack`] cache key, or `None` when constructed directly.
    tag: Option<&'static str>,
}

impl core::fmt::Debug for UndoStack {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UndoStack")
            .field("index", &self.index.get())
            .field("len", &self.commands.borrow().len())
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

impl Default for UndoStack {
    fn default() -> Self {
        Self::new()
    }
}

impl UndoStack {
    /// An empty, unbounded stack.
    #[must_use]
    pub fn new() -> Self {
        Self {
            commands: RefCell::new(Vec::new()),
            index: Cell::new(0),
            capacity: None,
            revision: Signal::new(0),
            tag: None,
        }
    }

    /// An empty stack bounded to at most `capacity` commands (the oldest is
    /// dropped from the front when the bound is exceeded). A `capacity` of 0
    /// is treated as 1 (a stack that can hold no command is degenerate).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: Some(capacity.max(1)),
            ..Self::new()
        }
    }

    /// As [`new`](Self::new) but records the [`use_undo_stack`] cache key,
    /// for symmetry with the other shared-state holders.
    #[must_use]
    pub fn with_tag(tag: &'static str) -> Self {
        Self {
            tag: Some(tag),
            ..Self::new()
        }
    }

    /// The [`use_undo_stack`] cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Bump the reactive revision (notifies every subscribed view). Reads
    /// the cell through `set_with` (no self-subscription).
    fn bump(&self) {
        self.revision.set_with(|v| v.wrapping_add(1));
    }

    /// Subscribe the current reactive scope (if any) to stack mutations.
    fn subscribe(&self) {
        let _ = self.revision.get();
    }

    /// Record and apply `command`: truncate the redo suffix, apply the
    /// command forward ([`redo`](UndoCommand::redo)), append it, and advance
    /// the cursor. Enforces [`capacity`](Self::with_capacity). This is the
    /// single mutation path — the edit is applied here, so callers build the
    /// command (snapshotting `before`) and hand it over, never mutating the
    /// target directly.
    pub fn record(&self, command: impl UndoCommand + 'static) {
        let boxed: Box<dyn UndoCommand> = Box::new(command);
        boxed.redo();
        let new_index = {
            let mut commands = self.commands.borrow_mut();
            let cursor = self.index.get();
            commands.truncate(cursor);
            commands.push(boxed);
            if let Some(cap) = self.capacity {
                while commands.len() > cap {
                    commands.remove(0);
                }
            }
            commands.len()
        };
        self.index.set(new_index);
        self.bump();
    }

    /// Step the cursor back one command, replaying its inverse. Returns
    /// `false` (a no-op) when already at the bottom of the stack.
    pub fn undo(&self) -> bool {
        let cursor = self.index.get();
        if cursor == 0 {
            return false;
        }
        {
            let commands = self.commands.borrow();
            commands[cursor - 1].undo();
        }
        self.index.set(cursor - 1);
        self.bump();
        true
    }

    /// Step the cursor forward one command, re-applying it. Returns `false`
    /// (a no-op) when already at the top of the stack.
    pub fn redo(&self) -> bool {
        let cursor = self.index.get();
        let len = self.commands.borrow().len();
        if cursor >= len {
            return false;
        }
        {
            let commands = self.commands.borrow();
            commands[cursor].redo();
        }
        self.index.set(cursor + 1);
        self.bump();
        true
    }

    /// Drop the entire history (cursor and both branches). Bumps the
    /// revision so dependent views repaint.
    pub fn clear(&self) {
        self.commands.borrow_mut().clear();
        self.index.set(0);
        self.bump();
    }

    /// Whether [`undo`](Self::undo) would step (reactive read).
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.subscribe();
        self.index.get() > 0
    }

    /// Whether [`redo`](Self::redo) would step (reactive read).
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.subscribe();
        self.index.get() < self.commands.borrow().len()
    }

    /// The cursor — the count of applied commands (reactive read).
    #[must_use]
    pub fn index(&self) -> usize {
        self.subscribe();
        self.index.get()
    }

    /// Total recorded commands across both branches (reactive read).
    #[must_use]
    pub fn len(&self) -> usize {
        self.subscribe();
        self.commands.borrow().len()
    }

    /// Whether the history is empty (reactive read).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The label of the command [`undo`](Self::undo) would replay, or `None`
    /// at the bottom of the stack (reactive read).
    #[must_use]
    pub fn undo_label(&self) -> Option<Cow<'static, str>> {
        self.subscribe();
        let cursor = self.index.get();
        (cursor > 0).then(|| self.commands.borrow()[cursor - 1].label())
    }

    /// The label of the command [`redo`](Self::redo) would re-apply, or
    /// `None` at the top of the stack (reactive read).
    #[must_use]
    pub fn redo_label(&self) -> Option<Cow<'static, str>> {
        self.subscribe();
        let cursor = self.index.get();
        let commands = self.commands.borrow();
        (cursor < commands.len()).then(|| commands[cursor].label())
    }

    /// Every command's label, bottom → top (the undo-history panel order;
    /// reactive read). `commands[..index()]` are applied.
    #[must_use]
    pub fn labels(&self) -> Vec<Cow<'static, str>> {
        self.subscribe();
        self.commands.borrow().iter().map(|c| c.label()).collect()
    }
}

/// R748 §5.52 — resolve the shared [`UndoStack`] for `key`, building an
/// unbounded one once. Mirrors [`use_scroll_state`](crate::widgets::scroll::use_scroll_state):
/// the reducer (which records edits), the view (which reads `can_undo`), and
/// the [`UndoStackExternal`] all call this with the same `key` and receive
/// the same `Rc`, so the history is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// reducer / a `create_extra_externals` hook — all run inside a
/// `root_owner.run`).
#[must_use]
pub fn use_undo_stack(key: &'static str) -> Rc<UndoStack> {
    Owner::current()
        .expect("use_undo_stack requires an active Owner scope")
        .cache(key, || UndoStack::with_tag(key))
}

/// R748 §5.52 §5.12 — the undo-history **coordinator** External: a thin
/// adapter that surfaces the shared [`UndoStack`] to the `scene/query` /
/// `scene/invoke` paths.
///
/// Like [`ViewSortFilterExternal`](crate::widgets::view_order) it owns no
/// interaction statechart and emits **no** §5.20 intent — undo/redo are
/// state mutations observed through `query` (`can_undo` / `index` /
/// `undo_label` / …), driven through the AI-first `invoke "undo"` /
/// `invoke "redo"` channel. All state lives in the shared [`UndoStack`]; the
/// external holds only the `Rc`.
#[derive(Clone)]
pub struct UndoStackExternal {
    stack: Rc<UndoStack>,
}

impl core::fmt::Debug for UndoStackExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UndoStackExternal")
            .field("index", &self.stack.index())
            .field("len", &self.stack.len())
            .finish_non_exhaustive()
    }
}

impl UndoStackExternal {
    /// Wrap the shared [`UndoStack`] (from [`use_undo_stack`]).
    #[must_use]
    pub fn new(stack: Rc<UndoStack>) -> Self {
        Self { stack }
    }

    /// The shared stack handle (the reducer + view reach the same `Rc` via
    /// [`use_undo_stack`]).
    #[must_use]
    pub fn stack(&self) -> &Rc<UndoStack> {
        &self.stack
    }

    /// The current cursor as an `IntrospectValue::Int` — the uniform return
    /// for the mutating `invoke` paths.
    fn index_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.stack.index()).unwrap_or(i64::MAX))
    }

    /// A label `Option` as `Text` / `Null`.
    fn label_value(label: Option<Cow<'static, str>>) -> IntrospectValue {
        label.map_or(IntrospectValue::Null, |l| IntrospectValue::Text(l.into_owned()))
    }
}

impl External for UndoStackExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }

    // A history coordinator emits no §5.20 intent (see the type doc); the
    // revision `Signal` write already repaints every subscribed view.
}

impl ExternalIntrospect for UndoStackExternal {
    fn schema(&self) -> IntrospectSchema {
        // `can_undo`/`can_redo` — whether a step is available (query only).
        // `index`            — applied-command cursor (query only).
        // `count`            — total recorded commands (query only).
        // `undo_label`/`redo_label` — next step's label, or Null (query).
        // `undo`/`redo`/`clear` — invoke channels.
        IntrospectSchema::new(&[
            ("can_undo", "bool"),
            ("can_redo", "bool"),
            ("index", "int"),
            ("count", "int"),
            ("undo_label", "string"),
            ("redo_label", "string"),
            ("undo", "bool"),
            ("redo", "bool"),
            ("clear", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // Every slot delegates to the shared [`UndoStack`]'s public API —
        // the single source of truth for "what does can_undo / a label
        // mean". The reactive accessors are safe to call from the RPC path:
        // `Signal::get` with no current owner simply does not subscribe and
        // returns the value (so there is no separate non-reactive branch to
        // keep in step).
        match path {
            "can_undo" => Some(IntrospectValue::Bool(self.stack.can_undo())),
            "can_redo" => Some(IntrospectValue::Bool(self.stack.can_redo())),
            "index" => Some(self.index_value()),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.stack.len()).unwrap_or(i64::MAX),
            )),
            "undo_label" => Some(Self::label_value(self.stack.undo_label())),
            "redo_label" => Some(Self::label_value(self.stack.redo_label())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "can_undo" | "can_redo" | "index" | "count" | "undo_label" | "redo_label" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(&mut self, path: &str, _args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first undo/redo — return whether a step actually happened.
            "undo" => Ok(IntrospectValue::Bool(self.stack.undo())),
            "redo" => Ok(IntrospectValue::Bool(self.stack.redo())),
            // Drop the whole history; returns the resulting cursor (0).
            "clear" => {
                self.stack.clear();
                Ok(self.index_value())
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A counter `Signal` + its stack, in a fresh `Owner` scope (so
    /// `Signal::set`'s owner lookups have a scope to find).
    fn fixture() -> (Signal<i64>, UndoStack) {
        (Signal::new(0), UndoStack::new())
    }

    #[test]
    fn record_applies_and_advances_cursor() {
        Owner::new().run(|| {
            let (counter, stack) = fixture();
            stack.record(SignalEdit::to(&counter, 1, "inc"));
            assert_eq!(counter.get(), 1, "record applies the edit");
            assert_eq!(stack.index(), 1);
            assert_eq!(stack.len(), 1);
            assert!(stack.can_undo());
            assert!(!stack.can_redo());
        });
    }

    #[test]
    fn undo_restores_before_redo_restores_after() {
        Owner::new().run(|| {
            let (counter, stack) = fixture();
            stack.record(SignalEdit::to(&counter, 5, "set 5"));
            assert!(stack.undo());
            assert_eq!(counter.get(), 0, "undo restores the before snapshot");
            assert!(!stack.can_undo());
            assert!(stack.can_redo());
            assert!(stack.redo());
            assert_eq!(counter.get(), 5, "redo restores the after snapshot");
        });
    }

    #[test]
    fn undo_redo_at_boundaries_are_noops() {
        Owner::new().run(|| {
            let (_counter, stack) = fixture();
            assert!(!stack.undo(), "undo on empty stack is a no-op");
            assert!(!stack.redo(), "redo on empty stack is a no-op");
        });
    }

    #[test]
    fn record_truncates_the_redo_branch() {
        Owner::new().run(|| {
            let (counter, stack) = fixture();
            stack.record(SignalEdit::to(&counter, 1, "a"));
            stack.record(SignalEdit::to(&counter, 2, "b"));
            stack.undo(); // back to 1; "b" is now redoable
            assert!(stack.can_redo());
            // A new edit truncates the redo branch.
            stack.record(SignalEdit::to(&counter, 9, "c"));
            assert_eq!(counter.get(), 9);
            assert!(!stack.can_redo(), "new edit dropped the redo branch");
            assert_eq!(stack.labels(), vec!["a", "c"], "history is a, c");
        });
    }

    #[test]
    fn heterogeneous_commands_share_one_timeline() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let title: Signal<String> = Signal::new(String::from("x"));
            let stack = UndoStack::new();
            stack.record(SignalEdit::to(&counter, 1, "inc"));
            stack.record(SignalEdit::to(&title, String::from("y"), "rename"));
            assert_eq!(stack.labels(), vec!["inc", "rename"]);
            // Undo unwinds in reverse: title first, then counter.
            stack.undo();
            assert_eq!(title.get(), "x", "title edit undone first");
            assert_eq!(counter.get(), 1, "counter edit still applied");
            stack.undo();
            assert_eq!(counter.get(), 0, "counter edit undone second");
        });
    }

    #[test]
    fn capacity_drops_the_oldest_command() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let stack = UndoStack::with_capacity(2);
            stack.record(SignalEdit::to(&counter, 1, "a"));
            stack.record(SignalEdit::to(&counter, 2, "b"));
            stack.record(SignalEdit::to(&counter, 3, "c")); // drops "a"
            assert_eq!(stack.labels(), vec!["b", "c"], "oldest command dropped");
            assert_eq!(stack.len(), 2);
            assert_eq!(stack.index(), 2);
        });
    }

    #[test]
    fn labels_track_the_next_step() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let stack = UndoStack::new();
            stack.record(SignalEdit::to(&counter, 1, "first"));
            stack.record(SignalEdit::to(&counter, 2, "second"));
            assert_eq!(stack.undo_label().as_deref(), Some("second"));
            assert_eq!(stack.redo_label(), None);
            stack.undo();
            assert_eq!(stack.undo_label().as_deref(), Some("first"));
            assert_eq!(stack.redo_label().as_deref(), Some("second"));
        });
    }

    #[test]
    fn external_query_and_invoke_round_trip() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let stack = Rc::new(UndoStack::new());
            stack.record(SignalEdit::to(&counter, 7, "set 7"));
            let mut ext = UndoStackExternal::new(Rc::clone(&stack));

            assert_eq!(ext.query("can_undo"), Some(IntrospectValue::Bool(true)));
            assert_eq!(ext.query("can_redo"), Some(IntrospectValue::Bool(false)));
            assert_eq!(ext.query("index"), Some(IntrospectValue::Int(1)));
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(1)));
            assert_eq!(
                ext.query("undo_label"),
                Some(IntrospectValue::Text(String::from("set 7"))),
            );
            assert_eq!(ext.query("redo_label"), Some(IntrospectValue::Null));

            // invoke undo → value reverts, cursor steps back.
            assert_eq!(ext.invoke("undo", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            assert_eq!(counter.get(), 0);
            assert_eq!(ext.query("index"), Some(IntrospectValue::Int(0)));
            // invoke redo → value re-applies.
            assert_eq!(ext.invoke("redo", IntrospectValue::Null), Ok(IntrospectValue::Bool(true)));
            assert_eq!(counter.get(), 7);
            // clear empties the history.
            assert_eq!(ext.invoke("clear", IntrospectValue::Null), Ok(IntrospectValue::Int(0)));
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(0)));

            assert_eq!(ext.intervene("index", IntrospectValue::Int(3)), Err(InterveneError::ReadOnly));
            assert_eq!(ext.invoke("frobnicate", IntrospectValue::Null), Err(InvokeError::UnknownPath));
        });
    }
}
