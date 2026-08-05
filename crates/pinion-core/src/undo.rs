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
//! - **Single linear branch.** Command merging / coalescing (typing collapses
//!   into one undo step, the defaulted [`merge`](UndoCommand::merge) trait
//!   method) landed R796 with text typing as its consumer; compound *macro*
//!   transactions (group N edits as one, the
//!   [`begin_macro`](UndoStack::begin_macro) /
//!   [`end_macro`](UndoStack::end_macro) pair folding into a `MacroCommand`)
//!   landed R903 with text find &amp; replace's *Replace All* as its first
//!   consumer. Both were the additive, backward-compatible extensions the
//!   original shape reserved ([[abstraction-needs-second-consumer]]).
//! - **Optional capacity.** A bounded stack drops the oldest command from
//!   the front when full (the `QUndoStack::setUndoLimit` model); the
//!   [`use_undo_stack`] hook builds an unbounded stack.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use crate::input::Modifiers;
use crate::reactive::{Owner, Signal};

/// A single reversible edit — the object-safe `QUndoCommand` peer.
///
/// [`redo`](Self::redo) applies the edit forward (it is also what
/// `UndoStack::push` calls to *first* apply a freshly recorded command,
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

    /// R796 §5.52 — coalescing downcast hook (the `QUndoCommand` `mergeWith`
    /// support). Default `None`: this command never participates in merging.
    /// A command that overrides [`merge`](Self::merge) returns `Some(self)`
    /// so the command it folds into can recover its concrete type — the
    /// MSRV-1.85 stand-in for upcasting `dyn UndoCommand` to `dyn Any`
    /// (trait upcasting stabilised only in 1.86).
    fn as_any(&self) -> Option<&dyn core::any::Any> {
        None
    }

    /// R796 §5.52 — try to fold the freshly recorded `next` command into
    /// `self` (the `QUndoCommand::mergeWith` peer). Return `true` when
    /// absorbed: `next` is then discarded and `self` already updated to span
    /// both edits, so the stack grows by zero and one Ctrl+Z reverses the
    /// whole run (the textbook "typing collapses into one undo step").
    /// Default: never merge — each [`record`](UndoStack::record) /
    /// [`push_applied`](UndoStack::push_applied) is its own step.
    /// Implementors recover `next`'s concrete type via [`as_any`](Self::as_any).
    fn merge(&mut self, _next: &dyn UndoCommand) -> bool {
        false
    }
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

/// R903 §5.52 — a **compound** [`UndoCommand`] grouping N child edits into one
/// reversible step (the `QUndoStack` macro the [`UndoStack::begin_macro`] /
/// [`UndoStack::end_macro`] pair folds a transaction into). `redo` replays the
/// children in record order; `undo` replays their inverses in **reverse**
/// order — the textbook nesting rule so an interleaved sequence (replace A,
/// replace B, replace C) unwinds C-then-B-then-A and the document returns to
/// the exact pre-macro state. Its [`label`](UndoCommand::label) is the macro's
/// (e.g. "Replace all"), keeping the grouped step introspectable as one entry
/// (§2 #7) rather than leaking N internal splices.
struct MacroCommand {
    children: Vec<Box<dyn UndoCommand>>,
    label: Cow<'static, str>,
}

impl UndoCommand for MacroCommand {
    fn label(&self) -> Cow<'static, str> {
        self.label.clone()
    }

    fn redo(&self) {
        for child in &self.children {
            child.redo();
        }
    }

    fn undo(&self) {
        for child in self.children.iter().rev() {
            child.undo();
        }
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
    /// R903 §5.52 — **open macro transaction** buffer (the `QUndoStack`
    /// `beginMacro`/`endMacro` peer). While a macro is open
    /// ([`macro_depth`](Self::macro_depth) `> 0`), every
    /// [`record`](Self::record) / [`push_applied`](Self::push_applied) appends
    /// here instead of the main [`commands`](Self::commands) timeline and does
    /// **not** bump the revision — so the N constituent edits stay invisible
    /// to `can_undo` / `index` until [`end_macro`](Self::end_macro) folds them
    /// into one `MacroCommand` step. The first consumer is text find &
    /// replace's *Replace All* (one Ctrl+Z reverses every replacement).
    macro_buffer: RefCell<Vec<Box<dyn UndoCommand>>>,
    /// R903 §5.52 — macro nesting depth. `begin_macro` increments,
    /// `end_macro` decrements; the buffered edits commit as one step only
    /// when it returns to `0`. Nested macros therefore flatten into the
    /// outermost step (Qt nests them as separate sub-steps; pinion flattens —
    /// the consumer-visible contract is identical for the single-level use).
    macro_depth: Cell<usize>,
    /// R903 §5.52 — label for the in-flight macro, captured at the outermost
    /// [`begin_macro`](Self::begin_macro) and applied to the folded
    /// `MacroCommand` at [`end_macro`](Self::end_macro).
    macro_label: RefCell<Cow<'static, str>>,
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
            macro_buffer: RefCell::new(Vec::new()),
            macro_depth: Cell::new(0),
            macro_label: RefCell::new(Cow::Borrowed("")),
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
        self.commit(Box::new(command), false);
    }

    /// R796 §5.52 — append a command whose forward effect the **caller has
    /// already applied**, skipping the initial [`redo`](UndoCommand::redo).
    ///
    /// The single mutation path of [`record`](Self::record) suits a reducer
    /// that hands over a not-yet-applied edit. A text field, by contrast,
    /// mutates its reactive buffer eagerly on every keystroke (selection
    /// drain, run clipping, caret advance) and only needs the *history*
    /// entry afterwards — re-running `redo` would redundantly re-set the
    /// signals. This records the already-applied edit, attempting the same
    /// coalescing merge with the top of the done prefix so a typing run
    /// folds into one undo step.
    pub fn push_applied(&self, command: impl UndoCommand + 'static) {
        self.commit(Box::new(command), true);
    }

    /// R903 §5.52 — open a **macro transaction** (the `QUndoStack::beginMacro`
    /// peer). Until the matching [`end_macro`](Self::end_macro), every
    /// [`record`](Self::record) / [`push_applied`](Self::push_applied) is
    /// buffered instead of landing on the timeline, so the constituent edits
    /// collapse into one undo step. `label` describes that step (e.g. "Replace
    /// all"); it is captured only at the **outermost** `begin_macro` — nested
    /// macros flatten into the outermost one (a `begin_macro` while a macro is
    /// already open just deepens the nesting). The buffered edits are still
    /// applied **eagerly** by their callers (the text mutators write the
    /// reactive buffer on each splice); the stack only withholds the *history*
    /// entries, so the document mutates live during the macro and only the
    /// Ctrl+Z granularity is grouped.
    pub fn begin_macro(&self, label: impl Into<Cow<'static, str>>) {
        if self.macro_depth.get() == 0 {
            self.macro_buffer.borrow_mut().clear();
            *self.macro_label.borrow_mut() = label.into();
        }
        self.macro_depth.set(self.macro_depth.get() + 1);
    }

    /// R903 §5.52 — close the macro opened by [`begin_macro`](Self::begin_macro)
    /// (the `QUndoStack::endMacro` peer). A no-op when no macro is open. When
    /// the **outermost** macro closes, the buffered edits fold into one history
    /// step: an empty buffer records nothing (an all-no-op macro leaves the
    /// timeline untouched) and one or more fold into a `MacroCommand`
    /// carrying the macro `label`. A single-child macro still wraps, so the
    /// step reads as "Replace all" rather than the lone child's "Replace" — the
    /// macro name is the introspectable truth of what the user did. Its `undo`
    /// reverses the children in reverse order. The folded step is pushed
    /// already-applied (the callers applied each child eagerly) and bumps the
    /// revision once, so dependent views see exactly one transition.
    pub fn end_macro(&self) {
        let depth = self.macro_depth.get();
        if depth == 0 {
            return;
        }
        self.macro_depth.set(depth - 1);
        if depth > 1 {
            return;
        }
        let children = std::mem::take(&mut *self.macro_buffer.borrow_mut());
        if children.is_empty() {
            return;
        }
        let label = self.macro_label.borrow().clone();
        self.commit(Box::new(MacroCommand { children, label }), true);
    }

    /// Shared body of [`record`](Self::record) / [`push_applied`](Self::push_applied):
    /// truncate the redo suffix, attempt to coalesce into the top command,
    /// else append (honouring [`capacity`](Self::with_capacity)). `applied`
    /// skips the initial forward application (the caller already performed it).
    fn commit(&self, boxed: Box<dyn UndoCommand>, applied: bool) {
        if !applied {
            boxed.redo();
        }
        // R903 §5.52 — an open macro diverts the edit into the macro buffer:
        // it stays out of the visible timeline (no append, no coalesce, no
        // revision bump) until `end_macro` folds the run into one step.
        if self.macro_depth.get() > 0 {
            self.macro_buffer.borrow_mut().push(boxed);
            return;
        }
        let mut commands = self.commands.borrow_mut();
        let cursor = self.index.get();
        // Drop any redone-then-superseded suffix. A non-empty suffix means
        // the user undid and then made a fresh edit, so the surviving top is
        // *not* part of the current typing run — skip coalescing in that case.
        let had_suffix = commands.len() > cursor;
        commands.truncate(cursor);
        if !had_suffix && cursor > 0 {
            if let Some(top) = commands.last_mut() {
                if top.merge(boxed.as_ref()) {
                    drop(commands);
                    self.bump();
                    return;
                }
            }
        }
        commands.push(boxed);
        if let Some(cap) = self.capacity {
            while commands.len() > cap {
                commands.remove(0);
            }
        }
        let new_index = commands.len();
        drop(commands);
        self.index.set(new_index);
        self.bump();
    }

    /// Step the cursor back one command, replaying its inverse. Returns
    /// `false` (a no-op) when already at the bottom of the stack.
    pub fn undo(&self) -> bool {
        // R906 — an open macro buffers edits off the timeline; stepping the
        // cursor mid-macro would commit the folded step at the wrong index and
        // truncate live history. Fail loud in dev, no-op in release.
        if self.macro_depth.get() > 0 {
            debug_assert!(false, "undo() called while a macro transaction is open");
            return false;
        }
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
        // R906 — see [`undo`](Self::undo): redo is undefined mid-macro.
        if self.macro_depth.get() > 0 {
            debug_assert!(false, "redo() called while a macro transaction is open");
            return false;
        }
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

/// R932 §5.52 — map a keystroke to an undo-stack verb (`"undo"` / `"redo"`), the
/// canonical editor pairing every undo-driving widget shares: `Ctrl`/`Cmd` + `Z`
/// undoes, `Ctrl`/`Cmd` + `Shift` + `Z` or `Ctrl`/`Cmd` + `Y` redoes; any other
/// combination is `None` (a non-undo key, so the caller's plain-key handling
/// runs). The verb is the [`UndoStackExternal`] invoke path, so a consumer
/// driving the AI-first surface forwards it verbatim
/// (`introspect.invoke(verb, …)`), while one holding an [`UndoStack`] directly
/// matches on it (`"redo" => stack.redo()`).
///
/// Lifted across the node-graph / data-grid / tree-grid / text-field undo
/// keymaps: the key → verb decision is the same everywhere because a divergence
/// would be an inconsistent editor keybinding — a bug — so it is one SSOT, not a
/// per-widget copy. (The hand-rolled text-field copy had already diverged: it
/// matched a lowercase `"z"` literally and so missed the platform's uppercase
/// `"Z"` on `Shift`, silently dropping `Ctrl+Shift+Z` redo — the exact
/// inconsistency the single source removes.)
#[must_use]
pub fn undo_redo_verb(key: &str, modifiers: Modifiers) -> Option<&'static str> {
    if !modifiers.command_key() {
        return None;
    }
    match key.to_ascii_lowercase().as_str() {
        "z" if modifiers.shift_key() => Some("redo"),
        "z" => Some("undo"),
        "y" => Some("redo"),
        _ => None,
    }
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
        label.map_or(IntrospectValue::Null, |l| {
            IntrospectValue::Text(l.into_owned())
        })
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
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("can_undo", "bool"),
                    SchemaField::new("can_redo", "bool"),
                    SchemaField::new("index", "int"),
                    SchemaField::new("count", "int"),
                    SchemaField::new("undo_label", "string"),
                    SchemaField::new("redo_label", "string"),
                    SchemaField::action("undo", "bool"),
                    SchemaField::action("redo", "bool"),
                    SchemaField::action("clear", "int"),
                ]
            },
        )
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

    fn invoke(
        &mut self,
        path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
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
    fn r903_macro_folds_children_into_one_step() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let stack = UndoStack::new();
            stack.begin_macro("batch +3");
            stack.record(SignalEdit::to(&counter, 1, "a"));
            stack.record(SignalEdit::to(&counter, 2, "b"));
            stack.record(SignalEdit::to(&counter, 3, "c"));
            // During the macro the children are eagerly applied but invisible
            // to the timeline.
            assert_eq!(counter.get(), 3, "children apply eagerly");
            assert_eq!(stack.index(), 0, "no step lands until end_macro");
            stack.end_macro();
            assert_eq!(stack.len(), 1, "three edits folded into one step");
            assert_eq!(
                stack.labels(),
                vec!["batch +3"],
                "macro label, not children"
            );
            // One Ctrl+Z reverses the whole run.
            assert!(stack.undo());
            assert_eq!(counter.get(), 0, "single undo reverses all three");
            assert!(stack.redo());
            assert_eq!(counter.get(), 3, "single redo re-applies all three");
        });
    }

    #[test]
    fn r903_macro_undo_unwinds_children_in_reverse() {
        Owner::new().run(|| {
            // Two signals prove the reverse-order replay: the macro sets a then
            // b; undo must restore b first then a, returning both to before.
            let a: Signal<i64> = Signal::new(10);
            let b: Signal<i64> = Signal::new(20);
            let stack = UndoStack::new();
            stack.begin_macro("set both");
            stack.record(SignalEdit::to(&a, 11, "set a"));
            stack.record(SignalEdit::to(&b, 21, "set b"));
            stack.end_macro();
            stack.undo();
            assert_eq!(a.get(), 10, "a restored");
            assert_eq!(b.get(), 20, "b restored");
        });
    }

    #[test]
    fn r903_empty_macro_records_no_step() {
        Owner::new().run(|| {
            let (_counter, stack) = fixture();
            stack.begin_macro("nothing");
            stack.end_macro();
            assert_eq!(
                stack.len(),
                0,
                "an empty macro leaves the timeline untouched"
            );
            assert!(!stack.can_undo());
        });
    }

    #[test]
    fn r903_single_child_macro_keeps_the_macro_label() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let stack = UndoStack::new();
            stack.begin_macro("Replace all");
            stack.record(SignalEdit::to(&counter, 1, "Replace"));
            stack.end_macro();
            assert_eq!(stack.len(), 1);
            assert_eq!(
                stack.labels(),
                vec!["Replace all"],
                "one-child macro reads as the macro, not the lone child"
            );
            assert!(stack.undo());
            assert_eq!(counter.get(), 0);
        });
    }

    #[test]
    fn r903_nested_macros_flatten_into_the_outermost_step() {
        Owner::new().run(|| {
            let counter: Signal<i64> = Signal::new(0);
            let stack = UndoStack::new();
            stack.begin_macro("outer");
            stack.record(SignalEdit::to(&counter, 1, "a"));
            stack.begin_macro("inner"); // nested: deepens, does not split
            stack.record(SignalEdit::to(&counter, 2, "b"));
            stack.end_macro(); // inner close: still one open level, no commit
            assert_eq!(stack.index(), 0, "inner end does not land a step");
            stack.record(SignalEdit::to(&counter, 3, "c"));
            stack.end_macro(); // outer close: one folded step
            assert_eq!(stack.len(), 1, "nested macros flatten to one step");
            assert_eq!(stack.labels(), vec!["outer"], "outermost label wins");
            stack.undo();
            assert_eq!(counter.get(), 0, "single undo reverses the flattened run");
        });
    }

    #[test]
    fn r903_end_macro_without_begin_is_a_noop() {
        Owner::new().run(|| {
            let (_counter, stack) = fixture();
            stack.end_macro(); // no open macro
            assert_eq!(stack.len(), 0);
        });
    }

    #[test]
    #[should_panic(expected = "macro transaction is open")]
    fn r906_undo_during_open_macro_panics_in_debug() {
        // R906 guard: stepping the cursor mid-macro would truncate live history.
        Owner::new().run(|| {
            let (_counter, stack) = fixture();
            stack.begin_macro("open");
            stack.undo(); // debug_assert fires (loud in dev)
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
            assert_eq!(
                ext.invoke("undo", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(counter.get(), 0);
            assert_eq!(ext.query("index"), Some(IntrospectValue::Int(0)));
            // invoke redo → value re-applies.
            assert_eq!(
                ext.invoke("redo", IntrospectValue::Null),
                Ok(IntrospectValue::Bool(true))
            );
            assert_eq!(counter.get(), 7);
            // clear empties the history.
            assert_eq!(
                ext.invoke("clear", IntrospectValue::Null),
                Ok(IntrospectValue::Int(0))
            );
            assert_eq!(ext.query("count"), Some(IntrospectValue::Int(0)));

            assert_eq!(
                ext.intervene("index", IntrospectValue::Int(3)),
                Err(InterveneError::ReadOnly)
            );
            assert_eq!(
                ext.invoke("frobnicate", IntrospectValue::Null),
                Err(InvokeError::UnknownPath)
            );
        });
    }

    // R796 §5.52 — coalescing + already-applied recording.

    /// Counts how many times `redo` fires, so a test can prove
    /// [`UndoStack::push_applied`] does **not** re-run the forward effect.
    struct CountCmd {
        redos: Rc<Cell<u32>>,
    }
    impl UndoCommand for CountCmd {
        fn label(&self) -> Cow<'static, str> {
            Cow::Borrowed("count")
        }
        fn redo(&self) {
            self.redos.set(self.redos.get() + 1);
        }
        fn undo(&self) {}
    }

    /// A set-to-`after` command that coalesces with a contiguous successor
    /// (`self.after == next.before`) while it stays `coalescable`.
    struct AddCmd {
        cell: Rc<Cell<i64>>,
        before: i64,
        after: i64,
        coalescable: bool,
    }
    impl UndoCommand for AddCmd {
        fn label(&self) -> Cow<'static, str> {
            Cow::Borrowed("add")
        }
        fn redo(&self) {
            self.cell.set(self.after);
        }
        fn undo(&self) {
            self.cell.set(self.before);
        }
        fn as_any(&self) -> Option<&dyn core::any::Any> {
            Some(self)
        }
        fn merge(&mut self, next: &dyn UndoCommand) -> bool {
            let Some(next) = next.as_any().and_then(|a| a.downcast_ref::<AddCmd>()) else {
                return false;
            };
            if !self.coalescable || self.after != next.before {
                return false;
            }
            self.after = next.after;
            self.coalescable = next.coalescable;
            true
        }
    }

    #[test]
    fn push_applied_does_not_rerun_redo() {
        Owner::new().run(|| {
            let redos = Rc::new(Cell::new(0u32));
            let stack = UndoStack::new();
            stack.record(CountCmd {
                redos: redos.clone(),
            });
            assert_eq!(redos.get(), 1, "record applies the edit (one redo)");
            stack.push_applied(CountCmd {
                redos: redos.clone(),
            });
            assert_eq!(
                redos.get(),
                1,
                "push_applied does NOT re-run redo (caller already applied)"
            );
            assert_eq!(stack.len(), 2, "both are recorded");
        });
    }

    #[test]
    fn push_applied_coalesces_contiguous_run() {
        Owner::new().run(|| {
            let cell = Rc::new(Cell::new(0i64));
            let stack = UndoStack::new();
            cell.set(1);
            stack.push_applied(AddCmd {
                cell: cell.clone(),
                before: 0,
                after: 1,
                coalescable: true,
            });
            cell.set(2);
            stack.push_applied(AddCmd {
                cell: cell.clone(),
                before: 1,
                after: 2,
                coalescable: true,
            });
            assert_eq!(
                stack.len(),
                1,
                "contiguous coalescable edits fold into one step"
            );
            assert_eq!(cell.get(), 2);
            assert!(stack.undo());
            assert_eq!(cell.get(), 0, "one undo reverses the whole coalesced run");
            assert!(stack.redo());
            assert_eq!(cell.get(), 2, "one redo re-applies the whole run");
        });
    }

    #[test]
    fn coalescing_breaks_when_not_contiguous_or_not_coalescable() {
        Owner::new().run(|| {
            let cell = Rc::new(Cell::new(0i64));
            let stack = UndoStack::new();
            // A non-coalescable command never absorbs its successor.
            cell.set(1);
            stack.push_applied(AddCmd {
                cell: cell.clone(),
                before: 0,
                after: 1,
                coalescable: false,
            });
            cell.set(2);
            stack.push_applied(AddCmd {
                cell: cell.clone(),
                before: 1,
                after: 2,
                coalescable: true,
            });
            assert_eq!(stack.len(), 2, "a non-coalescable top stays its own step");
        });
    }

    #[test]
    fn no_coalesce_across_an_undo_then_edit() {
        Owner::new().run(|| {
            let cell = Rc::new(Cell::new(0i64));
            let stack = UndoStack::new();
            cell.set(1);
            stack.push_applied(AddCmd {
                cell: cell.clone(),
                before: 0,
                after: 1,
                coalescable: true,
            });
            assert!(stack.undo(), "undo back to 0");
            // Typing after an undo starts a fresh step — the truncated suffix
            // means the surviving stack must not absorb this contiguous edit.
            cell.set(1);
            stack.push_applied(AddCmd {
                cell: cell.clone(),
                before: 0,
                after: 1,
                coalescable: true,
            });
            assert_eq!(
                stack.len(),
                1,
                "the prior command was truncated, not merged into"
            );
            assert!(!stack.can_redo(), "no dangling redo branch");
        });
    }

    #[test]
    fn undo_redo_verb_maps_the_canonical_editor_chords() {
        let ctrl = Modifiers {
            shift: false,
            ctrl: true,
            alt: false,
            meta: false,
        };
        let ctrl_shift = Modifiers {
            shift: true,
            ctrl: true,
            alt: false,
            meta: false,
        };
        let cmd = Modifiers {
            shift: false,
            ctrl: false,
            alt: false,
            meta: true,
        };
        assert_eq!(undo_redo_verb("z", ctrl), Some("undo"));
        assert_eq!(undo_redo_verb("Z", ctrl), Some("undo"), "case-insensitive");
        assert_eq!(
            undo_redo_verb("z", ctrl_shift),
            Some("redo"),
            "Ctrl+Shift+Z redoes"
        );
        assert_eq!(undo_redo_verb("y", ctrl), Some("redo"), "Ctrl+Y redoes");
        assert_eq!(
            undo_redo_verb("z", cmd),
            Some("undo"),
            "Cmd (meta) is command_key too"
        );
        assert_eq!(
            undo_redo_verb("z", Modifiers::empty()),
            None,
            "a bare z is not undo"
        );
        assert_eq!(
            undo_redo_verb("a", ctrl),
            None,
            "Ctrl+A is not an undo chord"
        );
    }
}
