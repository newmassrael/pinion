//! R1449 §5.27 §5.38 §5.40 — **the completion model** every typeahead needs:
//! a prefix, a match rule, and a cursor over the candidates the rule accepts.
//!
//! Qt's `QCompleter` is the reference. It is deliberately *not* a widget — it
//! attaches to a `QLineEdit` / `QTextEdit` / any input and answers three
//! questions: which candidates match what has been typed
//! (`setFilterMode` × `setCaseSensitivity`), which one is current
//! (`currentCompletion`), and how the answer should be presented
//! (`setCompletionMode`: a popup, an unfiltered popup, or completed inline).
//! pinion had all three answers hard-coded, six times over: every typeahead in
//! tree so far spells `label.to_lowercase().contains(&needle)` inline, so a
//! prefix-only search, a case-sensitive one, or an inline completion was not
//! *configurable* — it was unwritable without editing each consumer.
//!
//! This module is that model, and only that model. It owns no paint, no popup,
//! and no keyboard map: a binding decides where the candidates come from and
//! what a commit does, exactly as `QCompleter` leaves `activated()` to its
//! widget.
//!
//! ## Where pinion is better than the reference (§2 #7 + §2 #2)
//!
//! `QCompleter` exposes `currentCompletion()` and `completionCount()`, but the
//! *list* an agent would need lives in a `QAbstractItemView` it must reach into,
//! the inline completion lives in the widget's text selection, and the filter /
//! case / mode knobs are C++ setters with no wire form at all. Here the whole
//! model is scene-as-data: [`CompleterExternal`] answers `prefix`, `filter`,
//! `case`, `mode`, `completion_count`, `current`, `current_completion`,
//! `inline`, and `completion.<i>` over `scene/query`, and takes all four knobs
//! back over `scene/intervene` — so an AI client reads and drives the same
//! completion a human sees, in one vocabulary.
//!
//! ## The one cursor rule (both popup shapes, no branch)
//!
//! [`CompletionMode::UnfilteredPopup`] shows **every** candidate with the best
//! match marked current; the filtered modes show only accepted candidates. That
//! is one rule, not two: the cursor lands on *the first displayed candidate the
//! prefix accepts*. In a filtered list every entry accepts, so it lands on 0; in
//! an unfiltered list it lands on the best match. The mode changes what
//! [`CompletionState::completions`] contains, never how the cursor is chosen.
//!
//! ## Scope (honest boundaries)
//!
//! - **Flat candidates.** `QCompleter::splitPath` / `pathFromIndex` (tree /
//!   filesystem path completion) are **not** here. That is a different source
//!   model, not a different completion rule, and pinion has no consumer for it.
//! - **Inline completion is prefix-shaped.** [`CompletionState::inline_completion`]
//!   answers only when the current candidate genuinely starts with the typed
//!   prefix — a `Contains` match ("ran" inside "Cranberry") has no suffix to
//!   append, and inventing one would type text the user did not ask for.
//! - **The popup is the binding's.** This model says *what* to show and *which*
//!   is current; `hello-completer` owns the popup surface, and reuses the same
//!   `ListBoxExternal` + `dismiss_barrier` decomposition the comboboxes use.

use std::cell::RefCell;
use std::rc::Rc;

use crate::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, SchemaArg,
    SchemaField, query_proxy_external_impl,
};
use crate::reactive::{Owner, Signal};
use crate::widgets::order_memo::{OrderMemo, source_at_value};

/// R1449 §5.38 — where in a candidate the typed prefix must appear
/// (`QCompleter::setFilterMode`, i.e. `Qt::MatchStartsWith` /
/// `Qt::MatchContains` / `Qt::MatchEndsWith`).
///
/// An **empty** prefix is accepted by all three (every candidate starts with,
/// contains, and ends with `""`) — the "nothing typed yet, offer everything"
/// state, with no special case in the rule.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompletionFilter {
    /// The candidate begins with the prefix — the classic typeahead, and the
    /// only mode with a meaningful inline suffix.
    #[default]
    StartsWith,
    /// The prefix appears anywhere in the candidate (the rule every existing
    /// pinion typeahead hard-coded before this module).
    Contains,
    /// The candidate ends with the prefix (suffix search: file extensions).
    EndsWith,
}

impl CompletionFilter {
    /// The wire token (`"starts_with"` / `"contains"` / `"ends_with"`).
    #[must_use]
    pub const fn to_wire(self) -> &'static str {
        match self {
            Self::StartsWith => "starts_with",
            Self::Contains => "contains",
            Self::EndsWith => "ends_with",
        }
    }

    /// Parse a wire token; `None` when unrecognised (the caller reports a
    /// type error rather than silently picking a rule the client did not ask
    /// for).
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "starts_with" => Some(Self::StartsWith),
            "contains" => Some(Self::Contains),
            "ends_with" => Some(Self::EndsWith),
            _ => None,
        }
    }
}

/// R1449 §5.38 — whether case matters when matching
/// (`QCompleter::setCaseSensitivity`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompletionCase {
    /// `"app"` does not match `"Apple"`.
    Sensitive,
    /// `"app"` matches `"Apple"` — the default every pinion typeahead used
    /// before this module, via `str::to_lowercase` on both sides.
    #[default]
    Insensitive,
}

impl CompletionCase {
    /// The wire token (`"sensitive"` / `"insensitive"`).
    #[must_use]
    pub const fn to_wire(self) -> &'static str {
        match self {
            Self::Sensitive => "sensitive",
            Self::Insensitive => "insensitive",
        }
    }

    /// Parse a wire token; `None` when unrecognised.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "sensitive" => Some(Self::Sensitive),
            "insensitive" => Some(Self::Insensitive),
            _ => None,
        }
    }
}

/// R1449 §5.38 — how completions are presented
/// (`QCompleter::setCompletionMode`).
///
/// The mode selects what [`CompletionState::completions`] contains and whether
/// [`CompletionState::inline_completion`] answers; it never changes the match
/// rule itself.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompletionMode {
    /// A popup listing only the accepted candidates (`PopupCompletion`).
    #[default]
    Popup,
    /// A popup listing **every** candidate, with the best match current
    /// (`UnfilteredPopupCompletion`) — the shape that keeps the full set
    /// visible while still pointing at the likely answer.
    UnfilteredPopup,
    /// No popup: the input is completed in place and the appended part is the
    /// [`inline_completion`](CompletionState::inline_completion)
    /// (`InlineCompletion`).
    Inline,
}

impl CompletionMode {
    /// The wire token (`"popup"` / `"unfiltered_popup"` / `"inline"`).
    #[must_use]
    pub const fn to_wire(self) -> &'static str {
        match self {
            Self::Popup => "popup",
            Self::UnfilteredPopup => "unfiltered_popup",
            Self::Inline => "inline",
        }
    }

    /// Parse a wire token; `None` when unrecognised.
    #[must_use]
    pub fn from_wire(s: &str) -> Option<Self> {
        match s {
            "popup" => Some(Self::Popup),
            "unfiltered_popup" => Some(Self::UnfilteredPopup),
            "inline" => Some(Self::Inline),
            _ => None,
        }
    }

    /// Whether this mode presents a popup list (both popup shapes do; inline
    /// completion does not). The one predicate a binding needs to decide
    /// whether to paint a panel at all.
    #[must_use]
    pub const fn is_popup(self) -> bool {
        matches!(self, Self::Popup | Self::UnfilteredPopup)
    }

    /// Whether this mode filters the displayed list down to the accepted
    /// candidates. [`UnfilteredPopup`](Self::UnfilteredPopup) is the one mode
    /// that does not — it displays every candidate and only *marks* the match.
    #[must_use]
    pub const fn filters(self) -> bool {
        !matches!(self, Self::UnfilteredPopup)
    }
}

/// R1449 §5.38 — does `candidate` match `prefix` under `filter` + `case`?
///
/// The single match rule of this module: every list, cursor, and inline
/// completion is derived from this predicate, so a consumer that needs the
/// rule alone (a paint-time "is this row a match" tint) reads the same answer
/// the model does.
#[must_use]
pub fn completion_accepts(
    candidate: &str,
    prefix: &str,
    filter: CompletionFilter,
    case: CompletionCase,
) -> bool {
    match case {
        CompletionCase::Sensitive => accepts_exact(candidate, prefix, filter),
        // Simple Unicode lowercase on both sides — the folding every existing
        // pinion typeahead used, kept identical so migrating a consumer to this
        // module cannot change which rows it matched.
        CompletionCase::Insensitive => {
            accepts_exact(&candidate.to_lowercase(), &prefix.to_lowercase(), filter)
        }
    }
}

/// The case-folded core of [`completion_accepts`] — both sides already in the
/// same case.
fn accepts_exact(candidate: &str, prefix: &str, filter: CompletionFilter) -> bool {
    match filter {
        CompletionFilter::StartsWith => candidate.starts_with(prefix),
        CompletionFilter::Contains => candidate.contains(prefix),
        CompletionFilter::EndsWith => candidate.ends_with(prefix),
    }
}

/// R1449 §5.38 — the indices of the `count` candidates that
/// [`completion_accepts`], in ascending source order.
///
/// The free-function peer of [`CompletionState`] (the `search_matches` shape
/// [`row_search`](crate::widgets::row_search) uses), so a consumer can compute
/// a match set directly and the state can memoize the same computation.
#[must_use]
pub fn completion_matches<'a>(
    count: usize,
    prefix: &str,
    filter: CompletionFilter,
    case: CompletionCase,
    candidate: impl Fn(usize) -> &'a str,
) -> Vec<usize> {
    (0..count)
        .filter(|&i| completion_accepts(candidate(i), prefix, filter, case))
        .collect()
}

/// R1449 §5.38 — the part of `candidate` beyond the typed `prefix`: what an
/// inline completion would append.
///
/// `None` when `candidate` does not *begin* with `prefix` under `case` — a
/// `Contains` / `EndsWith` match has nothing to append, and an inline
/// completion that guessed would type text the user never asked for.
///
/// Under [`CompletionCase::Insensitive`] the boundary is found by scanning the
/// candidate's own char boundaries for the one whose folded prefix equals the
/// folded `prefix`, because a case fold is not length-preserving (`'İ'`
/// lowercases to two chars) — slicing `candidate` at `prefix.len()` would cut a
/// different place, or panic mid-char.
#[must_use]
pub fn completion_suffix<'a>(
    candidate: &'a str,
    prefix: &str,
    case: CompletionCase,
) -> Option<&'a str> {
    match case {
        CompletionCase::Sensitive => candidate.strip_prefix(prefix),
        CompletionCase::Insensitive => {
            let folded = prefix.to_lowercase();
            candidate
                .char_indices()
                .map(|(i, _)| i)
                .chain(std::iter::once(candidate.len()))
                .find(|&b| candidate[..b].to_lowercase() == folded)
                .map(|b| &candidate[b..])
        }
    }
}

/// The [`OrderMemo`] key: the displayed completion list depends on the prefix,
/// the match rule, and the mode (which decides whether the list is filtered at
/// all).
type CompletionKey = (String, CompletionFilter, CompletionCase, CompletionMode);

/// R1449 §5.27 §5.38 §5.40 — the reactive **single source of truth** for a
/// completion: the typed prefix, the three knobs, the cursor, and the memoized
/// displayed list.
///
/// Holds the candidates materialized once (the immutable source that makes the
/// value-keyed `OrderMemo` sound), each knob as a reactive [`Signal`], and a
/// `current` [`Signal`] cursor into the displayed list. Created once via
/// [`use_completion`] and shared — the same `Rc` — by the [`CompleterExternal`]
/// (which mutates) and the view (which reads), so what the popup paints and
/// what an agent queries can never disagree. Reading
/// [`prefix`](Self::prefix) / [`current_completion`](Self::current_completion)
/// inside a view-fn auto-subscribes, so a keystroke repaints exactly like any
/// other signal write.
pub struct CompletionState {
    tag: Option<&'static str>,
    /// The candidate strings, materialized once.
    candidates: Vec<String>,
    /// The typed prefix (`QCompleter::setCompletionPrefix`).
    prefix: Signal<String>,
    /// Where the prefix must appear in a candidate.
    filter: Signal<CompletionFilter>,
    /// Whether case matters.
    case: Signal<CompletionCase>,
    /// How completions are presented.
    mode: Signal<CompletionMode>,
    /// Cursor: a position in the **displayed** list, or `None` when nothing is
    /// current (no candidate accepts the prefix).
    current: Signal<Option<usize>>,
    /// Memoized displayed list, recomputed only when the key changes — the
    /// shared `OrderMemo` (R780 lift), 5th consumer.
    completions: RefCell<OrderMemo<CompletionKey>>,
}

impl CompletionState {
    /// Construct over `candidates`, with an empty prefix and the defaults every
    /// pinion typeahead already used (`Contains` was the hard-coded rule;
    /// `StartsWith` is the [`CompletionFilter`] default because it is the rule
    /// a *completer* — as opposed to a search box — is for, so a consumer that
    /// wants the old behaviour asks for it).
    #[must_use]
    pub fn new(candidates: Vec<String>) -> Self {
        let state = Self {
            tag: None,
            candidates,
            prefix: Signal::new(String::new()),
            filter: Signal::new(CompletionFilter::default()),
            case: Signal::new(CompletionCase::default()),
            mode: Signal::new(CompletionMode::default()),
            current: Signal::new(None),
            completions: RefCell::new(OrderMemo::new()),
        };
        state.land_cursor();
        state
    }

    /// As [`new`](Self::new) but records the [`use_completion`] cache key, for
    /// symmetry with [`RowSearchState::with_tag`](crate::widgets::row_search::RowSearchState::with_tag).
    #[must_use]
    pub fn with_tag(key: &'static str, candidates: Vec<String>) -> Self {
        Self {
            tag: Some(key),
            ..Self::new(candidates)
        }
    }

    /// The [`use_completion`] cache key, or `None` when constructed directly.
    #[must_use]
    pub fn tag(&self) -> Option<&'static str> {
        self.tag
    }

    /// Candidate count (prefix-independent).
    #[must_use]
    pub fn count(&self) -> usize {
        self.candidates.len()
    }

    /// The candidate text at source index `i`, or `""` out of range.
    #[must_use]
    pub fn candidate(&self, i: usize) -> &str {
        self.candidates.get(i).map_or("", String::as_str)
    }

    /// The typed prefix. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn prefix(&self) -> String {
        self.prefix.get()
    }

    /// Where the prefix must appear. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn filter(&self) -> CompletionFilter {
        self.filter.get()
    }

    /// Whether case matters. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn case(&self) -> CompletionCase {
        self.case.get()
    }

    /// How completions are presented. Subscribes when read inside a view-fn.
    #[must_use]
    pub fn mode(&self) -> CompletionMode {
        self.mode.get()
    }

    /// The **displayed** completion list as source indices: the accepted
    /// candidates under a filtering mode, every candidate under
    /// [`CompletionMode::UnfilteredPopup`]. Memoized on the
    /// `(prefix, filter, case, mode)` key; cheap `Rc` clone on a hit.
    /// Subscribes to all four signals, so a view that calls this repaints when
    /// any knob moves.
    #[must_use]
    pub fn completions(&self) -> Rc<Vec<usize>> {
        let key = (
            self.prefix.get(),
            self.filter.get(),
            self.case.get(),
            self.mode.get(),
        );
        let count = self.candidates.len();
        self.completions.borrow_mut().get(key.clone(), || {
            if key.3.filters() {
                completion_matches(count, &key.0, key.1, key.2, |i| self.candidate(i))
            } else {
                (0..count).collect()
            }
        })
    }

    /// Number of displayed completions (`QCompleter::completionCount`).
    #[must_use]
    pub fn completion_count(&self) -> usize {
        self.completions().len()
    }

    /// The cursor's position in the displayed list, or `None` when no candidate
    /// accepts the prefix (`QCompleter::currentRow`).
    #[must_use]
    pub fn current_index(&self) -> Option<usize> {
        self.current.get()
    }

    /// The **source** index the cursor points at, or `None`.
    #[must_use]
    pub fn current_source(&self) -> Option<usize> {
        let i = self.current.get()?;
        self.completions().get(i).copied()
    }

    /// The current candidate's text, or `None` (`QCompleter::currentCompletion`).
    #[must_use]
    pub fn current_completion(&self) -> Option<String> {
        self.current_source().map(|s| self.candidate(s).to_string())
    }

    /// The source index at displayed position `i`, or `None` out of range.
    #[must_use]
    pub fn completion_at(&self, i: usize) -> Option<usize> {
        self.completions().get(i).copied()
    }

    /// What an inline completion would append after the typed prefix, or `None`.
    ///
    /// Answers only in [`CompletionMode::Inline`] — the other modes present a
    /// popup and append nothing — and only when the current candidate genuinely
    /// begins with the prefix ([`completion_suffix`]). An empty string is a
    /// real answer (the prefix already spells the whole candidate) and is
    /// distinct from `None`.
    #[must_use]
    pub fn inline_completion(&self) -> Option<String> {
        if self.mode.get() != CompletionMode::Inline {
            return None;
        }
        let source = self.current_source()?;
        completion_suffix(self.candidate(source), &self.prefix.get(), self.case.get())
            .map(str::to_string)
    }

    /// Land the cursor on the first displayed candidate the prefix accepts, or
    /// `None` when none does.
    ///
    /// The one cursor rule, shared by both popup shapes: a filtered list
    /// accepts every entry so this is position 0, an unfiltered list lands on
    /// the best match. Called after every knob write, so the cursor is never
    /// stale against a list it no longer indexes.
    fn land_cursor(&self) {
        let (prefix, filter, case) = (self.prefix.get(), self.filter.get(), self.case.get());
        let list = self.completions();
        let pos = list
            .iter()
            .position(|&src| completion_accepts(self.candidate(src), &prefix, filter, case));
        self.current.set(pos);
    }

    /// Set the typed prefix and re-land the cursor. Returns the resulting
    /// [`completion_count`](Self::completion_count) — the "type a character,
    /// how many are left?" round-trip in one call.
    pub fn set_prefix(&self, prefix: &str) -> usize {
        self.prefix.set(prefix.to_string());
        self.land_cursor();
        self.completion_count()
    }

    /// Set where the prefix must appear and re-land the cursor. Returns the
    /// resulting [`completion_count`](Self::completion_count).
    pub fn set_filter(&self, filter: CompletionFilter) -> usize {
        self.filter.set(filter);
        self.land_cursor();
        self.completion_count()
    }

    /// Set case sensitivity and re-land the cursor. Returns the resulting
    /// [`completion_count`](Self::completion_count).
    pub fn set_case(&self, case: CompletionCase) -> usize {
        self.case.set(case);
        self.land_cursor();
        self.completion_count()
    }

    /// Set the presentation mode and re-land the cursor. Returns the resulting
    /// [`completion_count`](Self::completion_count) — which *changes* with the
    /// mode, because an unfiltered popup displays every candidate.
    pub fn set_mode(&self, mode: CompletionMode) -> usize {
        self.mode.set(mode);
        self.land_cursor();
        self.completion_count()
    }

    /// Move the cursor to the next displayed completion, wrapping; lands on the
    /// first when none is current. Returns the new
    /// [`current_completion`](Self::current_completion), or `None` when the
    /// list is empty.
    ///
    /// Walks the **displayed** list, so in an unfiltered popup it steps through
    /// every candidate — the arrow keys move within what the user can see.
    pub fn next(&self) -> Option<String> {
        let n = self.completions().len();
        if n == 0 {
            return None;
        }
        let next = self.current.get().map_or(0, |i| (i + 1) % n);
        self.current.set(Some(next));
        self.current_completion()
    }

    /// Mirror of [`next`](Self::next): previous displayed completion, wrapping
    /// to the last; lands on the last when none is current.
    pub fn prev(&self) -> Option<String> {
        let n = self.completions().len();
        if n == 0 {
            return None;
        }
        let prev = self.current.get().map_or(n - 1, |i| (i + n - 1) % n);
        self.current.set(Some(prev));
        self.current_completion()
    }

    /// Jump the cursor to displayed position `i` (`QCompleter::setCurrentRow`).
    /// Out of range is a no-op returning `None`, never a panic.
    pub fn jump_to(&self, i: usize) -> Option<String> {
        if i < self.completions().len() {
            self.current.set(Some(i));
            self.current_completion()
        } else {
            None
        }
    }
}

/// R1449 §5.38 — resolve the shared [`CompletionState`] for `key`, building it
/// once from `candidates`. Mirrors
/// [`use_row_search`](crate::widgets::row_search::use_row_search): the
/// `External` and the view both call this with the same `key` and receive the
/// same `Rc`, so the completion is one source of truth.
///
/// # Panics
///
/// Panics if no current [`Owner`] is set (call from within a `view` / a
/// `create_extra_externals` hook — both run inside a `root_owner.run`).
#[must_use]
pub fn use_completion(
    key: &'static str,
    candidates: impl FnOnce() -> Vec<String>,
) -> Rc<CompletionState> {
    Owner::current()
        .expect("use_completion requires an active Owner scope")
        .cache(key, || CompletionState::with_tag(key, candidates()))
}

/// R1449 §5.38 §2 #7 — the completion **coordinator** External: the wire
/// surface `QCompleter` never had.
///
/// Like [`RowSearchExternal`](crate::widgets::row_search::RowSearchExternal) it
/// owns no interaction statechart and emits no §5.20 intent — every mutation is
/// a `Signal` write that already repaints the subscribed view. All state lives
/// in the shared [`CompletionState`].
#[derive(Clone)]
pub struct CompleterExternal {
    state: Rc<CompletionState>,
}

impl core::fmt::Debug for CompleterExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompleterExternal")
            .field("prefix", &self.state.prefix())
            .field("filter", &self.state.filter())
            .field("case", &self.state.case())
            .field("mode", &self.state.mode())
            .field("completion_count", &self.state.completion_count())
            .field("current", &self.state.current_index())
            .finish_non_exhaustive()
    }
}

impl CompleterExternal {
    /// Wrap the shared [`CompletionState`] (from [`use_completion`]).
    #[must_use]
    pub fn new(state: Rc<CompletionState>) -> Self {
        Self { state }
    }

    /// The shared state handle (the view reaches the same `Rc` via
    /// [`use_completion`]).
    #[must_use]
    pub fn state(&self) -> &Rc<CompletionState> {
        &self.state
    }

    /// `completion_count` as an `IntrospectValue::Int` — the uniform return for
    /// the knob-setting `invoke` paths.
    fn count_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::try_from(self.state.completion_count()).unwrap_or(i64::MAX))
    }

    /// `current_completion` as an `IntrospectValue` — `Null` when nothing is
    /// current. The uniform return for the cursor-moving `invoke` paths.
    fn current_value(&self) -> IntrospectValue {
        self.state
            .current_completion()
            .map_or(IntrospectValue::Null, IntrospectValue::Text)
    }
}

// The shared display-config-proxy `External` skeleton (R847 SSOT): no §5.20
// intent — the signal writes already repaint every subscribed view.
query_proxy_external_impl!(CompleterExternal);

impl ExternalIntrospect for CompleterExternal {
    fn schema(&self) -> IntrospectSchema {
        // `prefix` / `filter` / `case` / `mode` — the four knobs (query + intervene).
        // `completion_count` / `current` / `current_completion` / `inline` / `count`
        //   — derived readouts (query only).
        // `completion.<i>` / `source.<i>` — the i-th displayed completion's text /
        //   source index (query only).
        // `set_prefix` / `next` / `prev` / `jump` — invoke channels.
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("prefix", "string"),
                    SchemaField::new("filter", "string"),
                    SchemaField::new("case", "string"),
                    SchemaField::new("mode", "string"),
                    SchemaField::new("completion_count", "int"),
                    SchemaField::new("current", "int"),
                    SchemaField::new("current_completion", "string"),
                    SchemaField::new("inline", "string"),
                    SchemaField::new("count", "int"),
                    SchemaField::parametric(
                        "completion.<i>",
                        "string",
                        const { &[SchemaArg::index("i", "completion_count")] },
                    ),
                    SchemaField::parametric(
                        "source.<i>",
                        "int",
                        const { &[SchemaArg::index("i", "completion_count")] },
                    ),
                    SchemaField::new("set_prefix", "string"),
                    SchemaField::new("next", "string"),
                    SchemaField::new("prev", "string"),
                    SchemaField::new("jump", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // `source.<i>` resolves the i-th displayed completion's source index;
        // out of range reports Null (present-but-empty), never absence — the
        // shared `source_at.<pos>` contract.
        if let Some(rest) = path.strip_prefix("source.") {
            return Some(source_at_value(rest, |i| self.state.completion_at(i)));
        }
        // `completion.<i>` is the same projection read as text.
        if let Some(rest) = path.strip_prefix("completion.") {
            let text = rest
                .parse::<usize>()
                .ok()
                .and_then(|i| self.state.completion_at(i))
                .map(|src| self.state.candidate(src).to_string());
            return Some(text.map_or(IntrospectValue::Null, IntrospectValue::Text));
        }
        match path {
            "prefix" => Some(IntrospectValue::Text(self.state.prefix())),
            "filter" => Some(IntrospectValue::Text(
                self.state.filter().to_wire().to_string(),
            )),
            "case" => Some(IntrospectValue::Text(
                self.state.case().to_wire().to_string(),
            )),
            "mode" => Some(IntrospectValue::Text(
                self.state.mode().to_wire().to_string(),
            )),
            "completion_count" => Some(self.count_value()),
            "current" => Some(
                self.state
                    .current_index()
                    .and_then(|i| i64::try_from(i).ok())
                    .map_or(IntrospectValue::Null, IntrospectValue::Int),
            ),
            "current_completion" => Some(self.current_value()),
            // `Null` when the mode appends nothing or the candidate does not
            // begin with the prefix; `""` when the prefix already spells it.
            "inline" => Some(
                self.state
                    .inline_completion()
                    .map_or(IntrospectValue::Null, IntrospectValue::Text),
            ),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.state.count()).unwrap_or(i64::MAX),
            )),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // The four knobs are the writable surface; an unrecognised token is a
        // TypeMismatch, not a silent fallback to some other rule.
        let IntrospectValue::Text(ref s) = value else {
            return match path {
                "prefix" | "filter" | "case" | "mode" => Err(InterveneError::TypeMismatch),
                "completion_count" | "current" | "current_completion" | "inline" | "count"
                | "completion" | "source" => Err(InterveneError::ReadOnly),
                _ => Err(InterveneError::UnknownPath),
            };
        };
        match path {
            "prefix" => {
                self.state.set_prefix(s);
                Ok(())
            }
            "filter" => {
                let f = CompletionFilter::from_wire(s).ok_or(InterveneError::TypeMismatch)?;
                self.state.set_filter(f);
                Ok(())
            }
            "case" => {
                let c = CompletionCase::from_wire(s).ok_or(InterveneError::TypeMismatch)?;
                self.state.set_case(c);
                Ok(())
            }
            "mode" => {
                let m = CompletionMode::from_wire(s).ok_or(InterveneError::TypeMismatch)?;
                self.state.set_mode(m);
                Ok(())
            }
            "completion_count" | "current" | "current_completion" | "inline" | "count"
            | "completion" | "source" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // AI-first typeahead: set the prefix and learn how many candidates
            // survive, in one round-trip.
            "set_prefix" => match args {
                IntrospectValue::Text(ref s) => {
                    self.state.set_prefix(s);
                    Ok(self.count_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Walk the displayed list; each returns the new current completion
            // (Null when the list is empty).
            "next" => {
                self.state.next();
                Ok(self.current_value())
            }
            "prev" => {
                self.state.prev();
                Ok(self.current_value())
            }
            // Jump to an explicit displayed position (out of range = no-op).
            // Like `next` / `prev`, the return is the cursor *readout* after the
            // call, not a success flag: an out-of-range jump reports the
            // unmoved current completion, because `Null` would claim there is
            // none. Callers that need "did it land" ask
            // [`CompletionState::jump_to`], whose `Option` says so.
            "jump" => match args {
                IntrospectValue::Int(i) => {
                    if let Ok(i) = usize::try_from(i) {
                        self.state.jump_to(i);
                    }
                    Ok(self.current_value())
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prefixes overlap ("A…", "B…", "Cr…") and one candidate contains another
    /// mid-word ("berry"), so the three filter modes give three different
    /// answers over the same list.
    const LABELS: [&str; 6] = [
        "Apple",
        "Apricot",
        "Banana",
        "Blueberry",
        "Cherry",
        "Cranberry",
    ];

    fn state() -> CompletionState {
        CompletionState::new(LABELS.iter().map(|s| (*s).to_string()).collect())
    }

    fn texts(s: &CompletionState) -> Vec<String> {
        s.completions()
            .iter()
            .map(|&i| s.candidate(i).to_string())
            .collect()
    }

    // ----- the match rule -----

    #[test]
    fn r1449_the_three_filters_are_three_different_answers() {
        let starts = completion_matches(
            6,
            "ber",
            CompletionFilter::StartsWith,
            CompletionCase::Insensitive,
            |i| LABELS[i],
        );
        let contains = completion_matches(
            6,
            "ber",
            CompletionFilter::Contains,
            CompletionCase::Insensitive,
            |i| LABELS[i],
        );
        let ends = completion_matches(
            6,
            "berry",
            CompletionFilter::EndsWith,
            CompletionCase::Insensitive,
            |i| LABELS[i],
        );
        assert!(starts.is_empty(), "no candidate begins with 'ber'");
        assert_eq!(contains, vec![3, 5], "Blueberry + Cranberry contain it");
        assert_eq!(ends, vec![3, 5], "and both end with 'berry'");
    }

    #[test]
    fn r1449_case_sensitivity_changes_the_match_set() {
        let insensitive = completion_matches(
            6,
            "ap",
            CompletionFilter::StartsWith,
            CompletionCase::Insensitive,
            |i| LABELS[i],
        );
        let sensitive = completion_matches(
            6,
            "ap",
            CompletionFilter::StartsWith,
            CompletionCase::Sensitive,
            |i| LABELS[i],
        );
        assert_eq!(insensitive, vec![0, 1], "Apple + Apricot");
        assert!(sensitive.is_empty(), "neither begins with a lowercase 'ap'");
    }

    #[test]
    fn r1449_an_empty_prefix_accepts_every_candidate_under_every_filter() {
        for filter in [
            CompletionFilter::StartsWith,
            CompletionFilter::Contains,
            CompletionFilter::EndsWith,
        ] {
            let all = completion_matches(6, "", filter, CompletionCase::Insensitive, |i| LABELS[i]);
            assert_eq!(all.len(), 6, "{filter:?}: an empty prefix filters nothing");
        }
    }

    // ----- the inline suffix -----

    #[test]
    fn r1449_the_suffix_is_what_inline_completion_would_append() {
        assert_eq!(
            completion_suffix("Apricot", "Ap", CompletionCase::Sensitive),
            Some("ricot")
        );
        assert_eq!(
            completion_suffix("Apricot", "ap", CompletionCase::Insensitive),
            Some("ricot"),
            "a case-insensitive match keeps the candidate's own case"
        );
        assert_eq!(
            completion_suffix("Apricot", "ap", CompletionCase::Sensitive),
            None,
            "case-sensitive: no prefix match, nothing to append"
        );
        assert_eq!(
            completion_suffix("Apricot", "Apricot", CompletionCase::Sensitive),
            Some(""),
            "the prefix already spells it: an empty suffix, not None"
        );
    }

    #[test]
    fn r1449_a_non_length_preserving_fold_does_not_cut_the_candidate_wrong() {
        // 'İ' (U+0130) lowercases to TWO chars, so a byte-length slice at
        // prefix.len() would land in the wrong place. The boundary scan does not.
        let candidate = "İstanbul";
        assert_eq!(
            completion_suffix(candidate, "i\u{307}", CompletionCase::Insensitive),
            Some("stanbul"),
            "the folded prefix spans one candidate char"
        );
        assert!(
            completion_suffix(candidate, "i\u{307}s", CompletionCase::Insensitive).is_some(),
            "and the scan keeps working past that char"
        );
    }

    // ----- the state + the one cursor rule -----

    #[test]
    fn r1449_a_filtered_popup_lands_the_cursor_on_the_first_match() {
        let s = state();
        assert_eq!(s.set_prefix("B"), 2);
        assert_eq!(texts(&s), vec!["Banana", "Blueberry"]);
        assert_eq!(s.current_index(), Some(0));
        assert_eq!(s.current_completion().as_deref(), Some("Banana"));
    }

    #[test]
    fn r1449_an_unfiltered_popup_shows_everything_and_marks_the_best_match() {
        let s = state();
        s.set_mode(CompletionMode::UnfilteredPopup);
        assert_eq!(s.set_prefix("B"), 6, "the list is the whole candidate set");
        assert_eq!(texts(&s).len(), 6);
        assert_eq!(
            s.current_index(),
            Some(2),
            "the cursor is on Banana's position in the FULL list"
        );
        assert_eq!(s.current_completion().as_deref(), Some("Banana"));
    }

    #[test]
    fn r1449_the_cursor_is_none_when_nothing_accepts_even_unfiltered() {
        let s = state();
        s.set_mode(CompletionMode::UnfilteredPopup);
        assert_eq!(s.set_prefix("zzz"), 6, "still displays everything");
        assert_eq!(
            s.current_index(),
            None,
            "but nothing is current — no candidate accepts the prefix"
        );
        assert_eq!(s.current_completion(), None);
    }

    #[test]
    fn r1449_switching_a_knob_relands_the_cursor_against_the_new_list() {
        let s = state();
        s.set_prefix("berry");
        assert_eq!(
            s.completion_count(),
            0,
            "StartsWith: nothing begins with it"
        );
        assert_eq!(s.current_index(), None);
        assert_eq!(s.set_filter(CompletionFilter::EndsWith), 2);
        assert_eq!(
            s.current_index(),
            Some(0),
            "the cursor re-lands on the new list, not on a stale index"
        );
        assert_eq!(s.current_completion().as_deref(), Some("Blueberry"));
    }

    #[test]
    fn r1449_the_cursor_walks_the_displayed_list_and_wraps() {
        let s = state();
        s.set_prefix("A");
        assert_eq!(s.current_completion().as_deref(), Some("Apple"));
        assert_eq!(s.next().as_deref(), Some("Apricot"));
        assert_eq!(s.next().as_deref(), Some("Apple"), "wraps to the first");
        assert_eq!(
            s.prev().as_deref(),
            Some("Apricot"),
            "wraps back to the last"
        );
        assert_eq!(s.jump_to(0).as_deref(), Some("Apple"));
        assert_eq!(s.jump_to(9), None, "out of range is a no-op");
        assert_eq!(
            s.current_completion().as_deref(),
            Some("Apple"),
            "and leaves the cursor where it was"
        );
    }

    #[test]
    fn r1449_inline_completion_answers_only_in_inline_mode() {
        let s = state();
        s.set_prefix("Ap");
        assert_eq!(s.inline_completion(), None, "a popup appends nothing");
        s.set_mode(CompletionMode::Inline);
        assert_eq!(s.inline_completion().as_deref(), Some("ple"));
        // A Contains match has no suffix to append — the honest boundary.
        s.set_filter(CompletionFilter::Contains);
        s.set_prefix("ran");
        assert_eq!(s.current_completion().as_deref(), Some("Cranberry"));
        assert_eq!(
            s.inline_completion(),
            None,
            "'ran' is inside Cranberry, not in front of it"
        );
    }

    #[test]
    fn r1449_an_empty_candidate_list_has_no_cursor_and_no_panic() {
        let s = CompletionState::new(Vec::new());
        assert_eq!(s.set_prefix("a"), 0);
        assert_eq!(s.current_index(), None);
        assert_eq!(s.next(), None);
        assert_eq!(s.prev(), None);
        assert_eq!(s.jump_to(0), None);
    }

    // ----- the wire surface -----

    #[test]
    fn r1449_every_knob_round_trips_through_its_wire_token() {
        for f in [
            CompletionFilter::StartsWith,
            CompletionFilter::Contains,
            CompletionFilter::EndsWith,
        ] {
            assert_eq!(CompletionFilter::from_wire(f.to_wire()), Some(f));
        }
        for c in [CompletionCase::Sensitive, CompletionCase::Insensitive] {
            assert_eq!(CompletionCase::from_wire(c.to_wire()), Some(c));
        }
        for m in [
            CompletionMode::Popup,
            CompletionMode::UnfilteredPopup,
            CompletionMode::Inline,
        ] {
            assert_eq!(CompletionMode::from_wire(m.to_wire()), Some(m));
        }
        assert_eq!(CompletionFilter::from_wire("startswith"), None);
        assert_eq!(CompletionMode::from_wire(""), None);
    }

    fn external() -> CompleterExternal {
        CompleterExternal::new(Rc::new(state()))
    }

    #[test]
    fn r1449_the_external_reads_and_drives_the_same_completion() {
        let mut ext = external();
        assert_eq!(
            ext.invoke("set_prefix", IntrospectValue::Text("A".into())),
            Ok(IntrospectValue::Int(2))
        );
        assert_eq!(
            ext.query("current_completion"),
            Some(IntrospectValue::Text("Apple".into()))
        );
        assert_eq!(
            ext.query("completion.1"),
            Some(IntrospectValue::Text("Apricot".into()))
        );
        assert_eq!(ext.query("source.1"), Some(IntrospectValue::Int(1)));
        assert_eq!(
            ext.query("completion.9"),
            Some(IntrospectValue::Null),
            "out of range is present-but-empty, never absence"
        );
        assert_eq!(
            ext.invoke("next", IntrospectValue::Null),
            Ok(IntrospectValue::Text("Apricot".into()))
        );
        assert_eq!(
            ext.invoke("jump", IntrospectValue::Int(0)),
            Ok(IntrospectValue::Text("Apple".into()))
        );
    }

    #[test]
    fn r1449_the_knobs_are_writable_and_a_bad_token_is_refused() {
        let mut ext = external();
        assert!(
            ext.intervene("mode", IntrospectValue::Text("unfiltered_popup".into()))
                .is_ok()
        );
        assert_eq!(
            ext.query("mode"),
            Some(IntrospectValue::Text("unfiltered_popup".into()))
        );
        assert_eq!(
            ext.intervene("filter", IntrospectValue::Text("nonsense".into())),
            Err(InterveneError::TypeMismatch),
            "an unknown rule is refused, not silently ignored"
        );
        assert_eq!(
            ext.query("filter"),
            Some(IntrospectValue::Text("starts_with".into())),
            "and the rule is unchanged"
        );
        assert_eq!(
            ext.intervene("completion_count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly),
            "a derived readout is read-only, not unknown"
        );
        assert_eq!(
            ext.intervene("nope", IntrospectValue::Text("x".into())),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn r1449_the_inline_readout_distinguishes_empty_from_absent() {
        let mut ext = external();
        ext.intervene("mode", IntrospectValue::Text("inline".into()))
            .expect("mode is writable");
        ext.intervene("prefix", IntrospectValue::Text("Apple".into()))
            .expect("prefix is writable");
        assert_eq!(
            ext.query("inline"),
            Some(IntrospectValue::Text(String::new())),
            "the prefix spells the whole candidate: nothing left to append"
        );
        ext.intervene("prefix", IntrospectValue::Text("zzz".into()))
            .expect("prefix is writable");
        assert_eq!(
            ext.query("inline"),
            Some(IntrospectValue::Null),
            "nothing is current: there is no completion at all"
        );
    }
}
