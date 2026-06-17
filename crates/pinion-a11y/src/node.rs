//! R51.61 §5.40 — Pinion `AccessNode` descriptor.
//!
//! [`AccessNode`] is the pinion-native a11y descriptor returned by
//! `WidgetView::access_node(&Scene, focused_tag) -> Option<AccessNode>`
//! (lands R51.63 wiring). It carries everything an
//! `accesskit::TreeUpdate` consumer needs to expose the widget to
//! Windows UIA / macOS AX / Linux AT-SPI / Android: ARIA role,
//! accessible name, current value, interaction-state flags, hit-test
//! bounds, and (for composites) the tag list of internal children.
//!
//! The struct is intentionally a plain data type — `AccessTreeBuilder`
//! (`tree.rs`) is the only consumer that lowers it into
//! `accesskit::Node`, so a future `accesskit` API change rewrites the
//! builder rather than every widget impl.

use pinion_core::scene::Rect;
use pinion_core::widgets::interaction::InteractionState;

use crate::role::{AriaCurrent, AriaRole, AutoComplete, HasPopup, SortDirection};

/// Pinion-native a11y descriptor for one widget.
///
/// One [`AccessNode`] per tagged widget in the paint scene. Composite
/// widgets (`RadioGroup`) own their internal children's `AccessNode`s
/// via `children: Vec<String>` (tag references — the tree builder
/// resolves them into `accesskit::NodeId`s at build time).
#[derive(Clone, Debug, PartialEq)]
pub struct AccessNode {
    /// Widget tag (the same identifier used by `InputRouter`, the
    /// focus manager, and the introspect schema).
    pub tag: String,
    /// ARIA role — drives `accesskit::Role` in the emitted node.
    pub role: AriaRole,
    /// Accessible name (`aria-label` equivalent).
    ///
    /// `WidgetView::access_node` impls leave this `None`; the shell
    /// calls [`crate::enrich_names_from_scene`] after layout to
    /// derive the name from the paint scene per WAI-ARIA 1.2 §4.3
    /// precedence: `ContainerNode::aria_label` override first, then
    /// the first descendant `TextNode::content`. Widgets that
    /// fundamentally lack visible text (icon-only without an
    /// `aria_label` modifier) may set this explicitly via
    /// [`AccessNode::with_name`] and the enrichment will respect
    /// that override.
    pub name: Option<String>,
    /// Current widget value (boolean for switch/check/radio, float
    /// for slider). Introspect schema reports the same value, by
    /// design (lockstep, single source of truth).
    pub value: Option<AccessValue>,
    /// Interaction-state flags — mirror §5.39 focus + §5.35 hover
    /// + §5.35 pressed.
    pub state: AccessState,
    /// Hit-test rectangle. Used by AT to overlay focus rings,
    /// magnifiers, and pointer-driven readout.
    ///
    /// The shell resolves this after layout from [`Self::tag`] (plus any
    /// [`Self::bounds_union_tags`]); a11y builders leave it `None`.
    pub bounds: Option<Rect>,
    /// R863 §5.40 §5.27 §5.45 — additional paint tags whose rects union
    /// into this node's resolved [`Self::bounds`].
    ///
    /// A node painted as a single fragment leaves this empty (the default):
    /// the shell resolves [`Self::bounds`] from [`Self::tag`] alone. A node
    /// whose visual extent is painted across **several** scene fragments —
    /// a frozen-split grid `Row` painted as `{tag}_row{id}` in the scrolling
    /// pane *and* `{tag}_frow{id}` in the frozen pane, or a tree-grid `row`
    /// painted as the `{tag}_drow{id}` metadata strip *and* the `{tag}#{id}`
    /// frozen name cell — lists the *other* fragment tags here, and the shell
    /// resolves [`Self::bounds`] as the union of `tag`'s rect with each
    /// resolvable union-tag rect (per [`Rect::union`](pinion_core::scene::Rect::union)).
    ///
    /// Only the fragments that resolve contribute: a union tag absent from
    /// the current paint scene is skipped, so the field is safe to populate
    /// unconditionally where the split may or may not be active. The
    /// frozen-pane span is the first consumer (the data-grid's `Row`); the
    /// tree-grid's `row` is the second, the divergence-is-a-bug trigger that
    /// lifts the union into the substrate rather than per-binding glue.
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714 /
    /// R717 / R730 / R731 / R739 additive-axis convention) so it defaults
    /// empty without forcing every hand-written node literal to enumerate it.
    pub bounds_union_tags: Vec<String>,
    /// Tag references for composite children. Empty for atomic
    /// widgets. The tree builder resolves these into
    /// `accesskit::NodeId`s and attaches them under this node.
    pub children: Vec<String>,
    /// R51.98 §5.40 — WAI-ARIA `aria-selected` per WAI-ARIA 1.2
    /// §6.6.7. `Some(true)` lowers to `accesskit::Node::set_selected`,
    /// `Some(false)` to `clear_selected` (explicit-unselected for AT
    /// awareness in multi-select containers), `None` omits the
    /// attribute (the default for roles without a selected semantic
    /// — `Button`, `Slider`, atomic `Switch`/`CheckBox`/`RadioButton`
    /// that already carry `aria-checked` instead).
    ///
    /// Axis distinction from `state.checked`: `aria-checked` is the
    /// truthy axis for two-state widgets (`Switch`, `CheckBox`,
    /// `RadioButton`); `aria-selected` is the truthy axis for
    /// container-membership widgets (`ListBoxOption`, `Tab`,
    /// `MenuItemRadio`, future grid cells). WAI-ARIA APG explicitly
    /// distinguishes them — a `Listbox` option is *selected*, not
    /// *checked*, regardless of the visual rendering. The R51.97
    /// `hello-listbox` emitted `aria-checked` via `state.checked` for
    /// `ListBox` options; R51.98 corrects that.
    pub selected: Option<bool>,
    /// R51.98 §5.40 — WAI-ARIA `aria-multiselectable` per
    /// WAI-ARIA 1.2 §6.6.6. `true` lowers to
    /// `accesskit::Node::set_multiselectable` (the AT then announces
    /// the container as "list, multi-selectable" instead of "list").
    /// Default `false` omits the attribute. Only meaningful on
    /// container roles that own a selection set (`Listbox`,
    /// future `Grid`/`Tree`/`TabList`); atomic roles ignore the flag.
    pub multiselectable: bool,
    /// R674 §5.40 — WAI-ARIA `aria-level` per WAI-ARIA 1.2 §6.6.8.
    /// One-based depth in the hierarchy. Required on per-item
    /// descriptors inside roles that own a hierarchical structure
    /// ([`AriaRole::TreeItem`] today; future `Heading` / `ListItem`
    /// nested under a `List`). The root of the hierarchy is
    /// `Some(1)`; each level of nesting adds one. `None` omits the
    /// attribute (the default for roles without a hierarchical
    /// semantic).
    ///
    /// **Authoring requirement** (WAI-ARIA 1.2 §6.6.8): for
    /// custom-widget roles without implicit native semantics
    /// (`role="treeitem"`, etc.) AT does **not** infer hierarchical
    /// depth from DOM nesting. Pinion paint scenes are flat row
    /// sequences (the substrate stamps composite tags per row), so
    /// the binding is the sole source of truth for the depth value.
    pub level: Option<u32>,
    /// R674 §5.40 — WAI-ARIA `aria-posinset` per WAI-ARIA 1.2
    /// §6.6.9. One-based position of this item within the parent's
    /// (visible) set. Pairs with [`Self::size_of_set`] so the AT
    /// can announce "item N of M".
    ///
    /// **Authoring requirement**: like [`Self::level`], the binding
    /// is the sole source of truth for custom-widget roles.
    /// `Some(1)` is the first sibling, `Some(2)` the second, etc.;
    /// `None` omits the attribute.
    pub position_in_set: Option<u32>,
    /// R674 §5.40 — WAI-ARIA `aria-setsize` per WAI-ARIA 1.2
    /// §6.6.10. Total count of (visible) items in this item's
    /// parent set. Pairs with [`Self::position_in_set`].
    ///
    /// **Authoring requirement**: when a tree / list owns a virtual
    /// or expandable set whose total count is unknown to the AT
    /// (collapsed branches, lazy-loaded children), the binding
    /// provides the visible-or-known total here. `None` omits the
    /// attribute.
    pub size_of_set: Option<u32>,
    /// R693 §5.40 — WAI-ARIA `aria-modal` per WAI-ARIA 1.2 §6.6.1.
    /// `true` lowers to `accesskit::Node::set_modal` so AT announces the
    /// node as a modal boundary and confines its virtual cursor to the
    /// subtree (the AT-side mirror of the [`crate::focus`]-trap the
    /// shell installs). Meaningful on [`AriaRole::Dialog`]; default
    /// `false` omits the attribute.
    pub modal: bool,
    /// R695 §5.40 — WAI-ARIA `aria-describedby` per WAI-ARIA 1.2
    /// §6.6.2. The tag of another [`AccessNode`] whose accessible name
    /// supplies *this* node's description (announced after the name).
    /// The tree builder resolves the tag into the target's
    /// `accesskit::NodeId` and lowers it via
    /// `accesskit::Node::set_described_by`. `None` omits the relation.
    ///
    /// The canonical use is the WCAG 2.2 SC 1.4.13 tooltip pattern: a
    /// trigger widget points its `described_by` at the
    /// [`AriaRole::Tooltip`] node so AT reads "Save, Saves the current
    /// file" — the tooltip text becomes the trigger's description, not
    /// a separately-focusable node. Single tag (not a list): pinion's
    /// one-description-source widgets need no multi-target relation
    /// until a 2nd consumer surfaces one
    /// (`[[abstraction-needs-second-consumer]]`).
    pub described_by: Option<String>,
    /// R696 §5.40 — WAI-ARIA 1.2 §6.6.3 `aria-expanded`, the truthy
    /// axis for **disclosure** controls (a `Button` whose activation
    /// shows / hides an associated content panel). `Some(true)` lowers
    /// to `accesskit::Node::set_expanded(true)` (shown), `Some(false)`
    /// to `set_expanded(false)` (collapsed), `None` omits the attribute
    /// (the default for roles without a disclosure semantic).
    ///
    /// Distinct axis from [`AccessState::checked`]: `aria-checked` is
    /// the on/off *value* of a two-state control (`Switch` / `CheckBox`
    /// / `RadioButton`); `aria-expanded` is whether a *separate*
    /// element this control governs is revealed. The disclosure /
    /// accordion pattern (R696) is the first consumer; a submenu title
    /// (WAI-ARIA §3.5; `role.rs` future axis) and a tree-row twisty
    /// ([`AriaRole::TreeItem`] future axis) are latent consumers.
    ///
    /// Placed on [`AccessNode`] (alongside [`Self::selected`] /
    /// [`Self::modal`]) rather than in [`AccessState`] so the additive
    /// axis defaults to absent in [`AccessNode::new`] without forcing
    /// every hand-written `AccessState { .. }` literal to enumerate it
    /// — the R674 / R693 / R695 additive-axis convention.
    ///
    /// [`AccessState::checked`]: crate::node::AccessState::checked
    /// [`AriaRole::TreeItem`]: crate::role::AriaRole::TreeItem
    pub expanded: Option<bool>,
    /// R714 §5.40 — WAI-ARIA 1.2 §6.6.3 `aria-controls`: the tag of
    /// another [`AccessNode`] whose presence / content *this* node
    /// governs. The tree builder resolves the tag into the target's
    /// `accesskit::NodeId` and lowers it via
    /// `accesskit::Node::push_controlled`. `None` omits the relation.
    ///
    /// The canonical first consumer is the WAI-ARIA §4.5 combobox
    /// pattern: a [`AriaRole::ComboBox`] trigger points its `controls`
    /// at the [`AriaRole::Listbox`] popup it opens, so AT announces the
    /// trigger/popup pairing even though they are sibling nodes. Single
    /// tag (not a list) per the [`Self::described_by`] precedent — a
    /// multi-target `aria-controls` waits for a 2nd consumer
    /// (`[[abstraction-needs-second-consumer]]`).
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696
    /// additive-axis convention) so it defaults absent without forcing
    /// every hand-written node literal to enumerate it.
    pub controls: Option<String>,
    /// R717 §5.40 — WAI-ARIA 1.2 §6.6.1 `aria-autocomplete`. Declares
    /// the completion behaviour of an editable combobox input; the tree
    /// builder lowers `Some(ac)` via `accesskit::Node::set_auto_complete`
    /// and `None` omits the attribute (`aria-autocomplete="none"`).
    ///
    /// The canonical consumer is the WAI-ARIA §4.5 editable combobox
    /// ([`AriaRole::EditableComboBox`]): the input carries
    /// `Some(AutoComplete::List)` so AT announces "editable, has popup,
    /// list autocomplete". Atomic and select-only roles leave it `None`.
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714
    /// additive-axis convention) so it defaults absent without forcing
    /// every hand-written node literal to enumerate it.
    pub auto_complete: Option<AutoComplete>,
    /// R730 §5.40 — WAI-ARIA 1.2 §6.6.2 `aria-sort` for a sortable
    /// [`AriaRole::ColumnHeader`]. `Some(dir)` lowers via
    /// `accesskit::Node::set_sort_direction`; `None` omits the attribute
    /// (`aria-sort="none"` — the column is sortable but not the current
    /// sort key, or not sortable at all). The data grid's sorted column
    /// header carries `Some(Ascending | Descending)`; every other header
    /// leaves it `None`.
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714 /
    /// R717 additive-axis convention) so it defaults absent without
    /// forcing every hand-written node literal to enumerate it.
    pub sort: Option<SortDirection>,
    /// R731 §5.40 — WAI-ARIA 1.2 §6.6.3 `aria-current`. `Some(kind)` marks
    /// this node as the current element of a related set (the breadcrumb's
    /// current crumb = `Some(AriaCurrent::Page)`); the tree builder lowers
    /// it via `accesskit::Node::set_aria_current`, and `None` omits the
    /// attribute (`aria-current="false"`).
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714 /
    /// R717 / R730 additive-axis convention) so it defaults absent without
    /// forcing every hand-written node literal to enumerate it.
    pub current: Option<AriaCurrent>,
    /// R739 §5.40 — WAI-ARIA 1.2 §6.6.2 `aria-valuetext`: a human-readable
    /// text alternative for [`AccessValue::Float`]'s numeric `aria-valuenow`.
    /// `Some(label)` lowers via `accesskit::Node::set_value` (the string
    /// value the labeled-step slider's "Medium" reads instead of "0.5"),
    /// while the numeric `aria-valuenow` / `valuemin` / `valuemax` still
    /// lower from the coexisting `AccessValue::Float` — the two are
    /// complementary, exactly as WAI-ARIA specifies (AT prefers the
    /// `valuetext` when present but keeps the numeric range for context).
    ///
    /// Distinct from [`AccessValue::Text`] (a text *field*'s entire value):
    /// `value_text` augments a *numeric* range widget (slider / spinbutton /
    /// progressbar) whose discrete stops carry names. Defaults `None`, so a
    /// continuous or plain-numeric range widget omits the attribute and AT
    /// announces the bare `valuenow`.
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714 /
    /// R717 / R730 / R731 additive-axis convention) so it defaults absent
    /// without forcing every hand-written node literal to enumerate it.
    pub value_text: Option<String>,
    /// R985 §5.40 — WAI-ARIA 1.2 §6.6.5 `aria-haspopup`. `Some(kind)` marks
    /// this node as the trigger that opens a popup of the given kind; the tree
    /// builder lowers it via `accesskit::Node::set_has_popup`, and `None` omits
    /// the attribute (`aria-haspopup="false"`).
    ///
    /// The canonical consumer is the WAI-ARIA §3.16 menubar submenu: a
    /// [`AriaRole::MenuItem`] that owns a child [`AriaRole::Menu`] carries
    /// `Some(HasPopup::Menu)` and pairs it with [`Self::expanded`] for the
    /// open / closed state.
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714 / R717 /
    /// R730 / R731 / R739 additive-axis convention) so it defaults absent
    /// without forcing every hand-written node literal to enumerate it.
    pub has_popup: Option<HasPopup>,
}

impl AccessNode {
    /// Construct a minimal node with no name / value / state /
    /// bounds / children. Builder-style setters (`with_*`) fill
    /// in the rest.
    #[must_use]
    pub fn new(tag: impl Into<String>, role: AriaRole) -> Self {
        Self {
            tag: tag.into(),
            role,
            name: None,
            value: None,
            state: AccessState::default(),
            bounds: None,
            bounds_union_tags: Vec::new(),
            children: Vec::new(),
            selected: None,
            multiselectable: false,
            level: None,
            position_in_set: None,
            size_of_set: None,
            modal: false,
            described_by: None,
            expanded: None,
            controls: None,
            auto_complete: None,
            sort: None,
            current: None,
            value_text: None,
            has_popup: None,
        }
    }

    /// Set the accessible name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the current value.
    #[must_use]
    pub fn with_value(mut self, value: AccessValue) -> Self {
        self.value = Some(value);
        self
    }

    /// Replace the state flags.
    #[must_use]
    pub fn with_state(mut self, state: AccessState) -> Self {
        self.state = state;
        self
    }

    /// Set the hit-test bounds.
    #[must_use]
    pub fn with_bounds(mut self, bounds: Rect) -> Self {
        self.bounds = Some(bounds);
        self
    }

    /// R863 §5.40 §5.27 — append a paint tag whose rect unions into this
    /// node's resolved [`Self::bounds`]. Call once per *additional* fragment
    /// the node is painted across (the node's own [`Self::tag`] is always the
    /// primary fragment, so it is never listed here). See
    /// [`Self::bounds_union_tags`] for the frozen-split / tree-grid span the
    /// substrate serves.
    #[must_use]
    pub fn with_bounds_union_tag(mut self, tag: impl Into<String>) -> Self {
        self.bounds_union_tags.push(tag.into());
        self
    }

    /// Append a composite child tag.
    #[must_use]
    pub fn with_child(mut self, child_tag: impl Into<String>) -> Self {
        self.children.push(child_tag.into());
        self
    }

    /// R51.98 §5.40 — set the WAI-ARIA `aria-selected` attribute.
    /// Use `true` for "this option is currently in the container's
    /// selection set", `false` for "explicitly not selected"
    /// (announced distinctly by AT in multi-select containers), or
    /// omit (leave `selected = None`) for roles that don't carry a
    /// selected axis.
    #[must_use]
    pub fn with_selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }

    /// R848 §5.50 — set the WAI-ARIA roving-cursor `focused` state (the
    /// `aria-activedescendant` target). `true` on the row a single-tab-stop
    /// composite currently addresses with its keyboard cursor, `false`
    /// otherwise — the same `state.focused` flag the listbox / radiogroup /
    /// menu builders set on their active descendant. Orthogonal to
    /// [`with_selected`](Self::with_selected): a grouped collection's cursor
    /// can rest on a group header (no selection) while a data row stays
    /// selected.
    #[must_use]
    pub fn with_focused(mut self, focused: bool) -> Self {
        self.state.focused = focused;
        self
    }

    /// R51.98 §5.40 — declare this container exposes
    /// `aria-multiselectable="true"`. Only meaningful on `Listbox`,
    /// future `Grid` / `Tree` / `TabList` parents; atomic roles
    /// ignore it at lowering.
    #[must_use]
    pub fn with_multiselectable(mut self) -> Self {
        self.multiselectable = true;
        self
    }

    /// R693 §5.40 — declare this node exposes `aria-modal="true"`. Used
    /// on the [`AriaRole::Dialog`] root while the dialog is open so AT
    /// confines its virtual cursor to the dialog subtree, mirroring the
    /// shell-side focus trap. See [`Self::modal`].
    #[must_use]
    pub fn with_modal(mut self) -> Self {
        self.modal = true;
        self
    }

    /// R674 §5.40 — set the WAI-ARIA `aria-level` attribute. See
    /// [`Self::level`] for the semantic axis and authoring contract.
    /// One-based: the root of the hierarchy is `1`.
    #[must_use]
    pub fn with_level(mut self, level: u32) -> Self {
        self.level = Some(level);
        self
    }

    /// R674 §5.40 — set the WAI-ARIA `aria-posinset` attribute. See
    /// [`Self::position_in_set`] for the semantic axis. One-based.
    #[must_use]
    pub fn with_position_in_set(mut self, position: u32) -> Self {
        self.position_in_set = Some(position);
        self
    }

    /// R674 §5.40 — set the WAI-ARIA `aria-setsize` attribute. See
    /// [`Self::size_of_set`] for the semantic axis.
    #[must_use]
    pub fn with_size_of_set(mut self, size: u32) -> Self {
        self.size_of_set = Some(size);
        self
    }

    /// R818 §5.40 — set both `aria-posinset` and `aria-setsize` from a
    /// zero-based `index` into a flat set of `len` items: `aria-posinset`
    /// is the one-based `index + 1`, `aria-setsize` is `len`.
    ///
    /// SSOTs the "position in a flat slice" derivation every cell-slice
    /// a11y builder shares (`listbox_option_nodes`, `tablist_tab_nodes`,
    /// `menu_item_nodes`, `grid_table_nodes` data rows, `toolbar_button_nodes`)
    /// — the one-based offset and the saturating `usize -> u32` conversion
    /// live here once instead of being re-derived per builder. (Builders
    /// whose position is *not* a flat slice index — `tree_view`'s
    /// per-sibling-group `VisibleRow`, the window-offset virtual lists —
    /// keep the individual setters.)
    #[must_use]
    pub fn with_set_position(mut self, index: usize, len: usize) -> Self {
        self.position_in_set = Some(u32::try_from(index + 1).unwrap_or(u32::MAX));
        self.size_of_set = Some(u32::try_from(len).unwrap_or(u32::MAX));
        self
    }

    /// R695 §5.40 — set the WAI-ARIA `aria-describedby` relation to the
    /// node tagged `tag`. See [`Self::described_by`] for the semantic
    /// axis (the tooltip-description pattern).
    #[must_use]
    pub fn with_described_by(mut self, tag: impl Into<String>) -> Self {
        self.described_by = Some(tag.into());
        self
    }

    /// R696 §5.40 — set the WAI-ARIA `aria-expanded` state. `true`
    /// marks the disclosure panel as shown, `false` as collapsed;
    /// leaving it unset (the default) omits the attribute for roles
    /// without a disclosure semantic. See [`Self::expanded`] for the
    /// axis distinction from `checked`.
    #[must_use]
    pub fn with_expanded(mut self, expanded: bool) -> Self {
        self.expanded = Some(expanded);
        self
    }

    /// R714 §5.40 — set the WAI-ARIA `aria-controls` relation to the
    /// node tagged `tag` (the combobox → listbox pairing). See
    /// [`Self::controls`] for the semantic axis.
    #[must_use]
    pub fn with_controls(mut self, tag: impl Into<String>) -> Self {
        self.controls = Some(tag.into());
        self
    }

    /// R717 §5.40 — set the WAI-ARIA `aria-autocomplete` value (the
    /// editable-combobox completion behaviour). See [`Self::auto_complete`].
    #[must_use]
    pub fn with_auto_complete(mut self, mode: AutoComplete) -> Self {
        self.auto_complete = Some(mode);
        self
    }

    /// R730 §5.40 — set the WAI-ARIA `aria-sort` direction on a sortable
    /// column header. See [`Self::sort`].
    #[must_use]
    pub fn with_sort(mut self, dir: SortDirection) -> Self {
        self.sort = Some(dir);
        self
    }

    /// R731 §5.40 — set the WAI-ARIA `aria-current` kind (the breadcrumb's
    /// current crumb). See [`Self::current`].
    #[must_use]
    pub fn with_current(mut self, kind: AriaCurrent) -> Self {
        self.current = Some(kind);
        self
    }

    /// R739 §5.40 — set the WAI-ARIA `aria-valuetext` label (the labeled-step
    /// slider's named stop, e.g. "Medium" for the numeric `valuenow` 0.5).
    /// Lowers alongside the coexisting [`AccessValue::Float`]. See
    /// [`Self::value_text`].
    #[must_use]
    pub fn with_value_text(mut self, text: impl Into<String>) -> Self {
        self.value_text = Some(text.into());
        self
    }

    /// R985 §5.40 — set the WAI-ARIA `aria-haspopup` kind (the submenu parent
    /// item's `HasPopup::Menu`). See [`Self::has_popup`].
    #[must_use]
    pub fn with_has_popup(mut self, kind: HasPopup) -> Self {
        self.has_popup = Some(kind);
        self
    }
}

/// Interaction-state flags exposed to AT.
///
/// Mirrors §5.39 focus + §5.35 hover / pressed; `disabled` is opt-in
/// (default = false, so widgets that ignore the disabled invariant
/// stay AT-active). `checked: Option<bool>` is `None` for widgets
/// without a check semantic (`Button`, `Slider`) and `Some` for
/// `Switch` / `CheckBox` / `Radio`.
///
/// The four flag bools mirror the WAI-ARIA 1.2 state vocabulary
/// (focused / disabled / hovered / pressed) one-to-one. A bitflags
/// refactor would compress storage but obscure the public surface
/// — the textbook ARIA presentation is named fields, so the
/// `clippy::struct_excessive_bools` pedantic threshold is overridden
/// here.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AccessState {
    pub focused: bool,
    pub disabled: bool,
    pub hovered: bool,
    pub pressed: bool,
    pub checked: Option<bool>,
}

impl AccessState {
    /// Build the disabled / hovered / pressed posture from a widget-state
    /// enum's [`InteractionState`] impl, plus the `aria-checked`/`aria-pressed`
    /// bit (`checked`). This is the R755 SSOT for the posture mapping that was
    /// previously hand-copied as `hovered: matches!(state, X::Hover)` … into
    /// 24 `access_node` sites across the gallery — the same posture the
    /// Material 3 state-layer overlay reads through the very same trait.
    ///
    /// `focused` is orthogonal (it comes from the focus manager / active
    /// descendant, not from the interaction enum) and stays at its `false`
    /// default; consumers that track focus override it via struct-update
    /// syntax: `AccessState { focused: …, ..AccessState::from_interaction(s, c) }`.
    #[must_use]
    pub fn from_interaction<S: InteractionState>(state: S, checked: Option<bool>) -> Self {
        Self {
            focused: false,
            disabled: state.is_disabled(),
            hovered: state.is_hovered(),
            pressed: state.is_pressed(),
            checked,
        }
    }
}

/// Numeric / boolean / string value carried by an `AccessNode`.
///
/// Lockstep with the introspect schema (§5.21): a checkbox's
/// `AccessValue::Bool` is the same `bool` the RPC introspect path
/// reports for the `"value"` key, and the slider's
/// `AccessValue::Float` shares min/max/value with the slider's
/// introspect descriptor.
#[derive(Clone, Debug, PartialEq)]
pub enum AccessValue {
    Bool(bool),
    Float { value: f32, min: f32, max: f32 },
    Text(String),
}

/// R980 §5.40 — attach a named `button` [`AccessNode`] as a child of an
/// existing node, the SSOT for an in-widget control affordance (a "reset to
/// default" / "remove" / "add element" button painted inside a row, cell, or
/// column header). Pushes `button_tag` onto the `parent_tag` node's children
/// (so AT announces it under its host) and appends the button node itself
/// (`role=button`, named). A `button` is a valid child of a `gridcell` /
/// `columnheader` / `treeitem` host (not of a bare grid `row`), so callers
/// attach it to a cell-level host.
///
/// The button is reachable by AT element navigation and activatable via an
/// `AccessKit` `Click` even though it is not a tab stop — the widget's
/// `WidgetView::access_child_invoke` routes the `Click` to the affordance's
/// action wire (the pointer twin).
///
/// **Orphan-free by construction (R984.1):** when `parent_tag` is absent from
/// `nodes` — e.g. its host row / cell is windowed out of a virtualized
/// projection — NOTHING is emitted (neither the child link nor the button
/// node), so a dangling AT node (a `button` no host references) can never
/// exist. Callers therefore need not pre-filter their affordances to present
/// hosts; passing an over-broad host set is self-correcting here rather than a
/// latent orphan. (Pre-R984.1 the node was pushed unconditionally and only the
/// link was guarded — the orphan the R983 grouped reset avoided by call-site
/// discipline; this makes the guarantee structural for every consumer.)
pub fn attach_child_button(
    nodes: &mut Vec<AccessNode>,
    parent_tag: &str,
    button_tag: String,
    name: String,
) {
    let Some(parent) = nodes.iter_mut().find(|n| n.tag == parent_tag) else {
        return;
    };
    parent.children.push(button_tag.clone());
    nodes.push(AccessNode::new(button_tag, AriaRole::Button).with_name(name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r818_with_set_position_is_one_based_posinset_plus_setsize() {
        // SSOT for the cell-slice builders: 0-based index -> 1-based
        // aria-posinset, plus aria-setsize = len.
        let first = AccessNode::new("a", AriaRole::ListBoxOption).with_set_position(0, 3);
        assert_eq!(first.position_in_set, Some(1));
        assert_eq!(first.size_of_set, Some(3));
        let last = AccessNode::new("c", AriaRole::ListBoxOption).with_set_position(2, 3);
        assert_eq!(last.position_in_set, Some(3));
        assert_eq!(last.size_of_set, Some(3));
    }

    #[test]
    fn attach_child_button_links_and_emits_under_a_present_host() {
        let mut nodes = vec![AccessNode::new("cell", AriaRole::GridCell)];
        attach_child_button(&mut nodes, "cell", "cell#reset".to_owned(), "Reset".to_owned());
        // The host now references the button, and the button node exists.
        let host = nodes.iter().find(|n| n.tag == "cell").expect("host present");
        assert!(host.children.contains(&"cell#reset".to_owned()), "host links the button");
        let btn = nodes.iter().find(|n| n.tag == "cell#reset").expect("button emitted");
        assert_eq!(btn.role, AriaRole::Button);
        assert_eq!(btn.name.as_deref(), Some("Reset"));
    }

    #[test]
    fn attach_child_button_emits_nothing_for_an_absent_host() {
        // R984.1 — orphan-free by construction: an affordance whose host is not
        // in the tree (windowed out) must leave NO dangling button node behind.
        let mut nodes = vec![AccessNode::new("other", AriaRole::GridCell)];
        attach_child_button(&mut nodes, "absent", "absent#reset".to_owned(), "Reset".to_owned());
        assert_eq!(nodes.len(), 1, "no button node is pushed when the host is absent");
        assert!(
            !nodes.iter().any(|n| n.tag == "absent#reset"),
            "the orphan button must not exist",
        );
        assert!(nodes[0].children.is_empty(), "no stray child link either");
    }

    #[test]
    fn new_starts_empty() {
        let n = AccessNode::new("main_btn", AriaRole::Button);
        assert_eq!(n.tag, "main_btn");
        assert_eq!(n.role, AriaRole::Button);
        assert!(n.name.is_none());
        assert!(n.value.is_none());
        assert!(n.bounds.is_none());
        assert!(n.children.is_empty());
        assert_eq!(n.state, AccessState::default());
        // R674 §5.40 — hierarchical axes default to absent so non-
        // tree/list roles continue to omit the attributes.
        assert!(n.level.is_none());
        assert!(n.position_in_set.is_none());
        assert!(n.size_of_set.is_none());
    }

    #[test]
    fn with_name_sets_name() {
        let n = AccessNode::new("btn", AriaRole::Button).with_name("Save");
        assert_eq!(n.name.as_deref(), Some("Save"));
    }

    #[test]
    fn with_value_bool() {
        let n = AccessNode::new("cb", AriaRole::CheckBox)
            .with_value(AccessValue::Bool(true));
        assert_eq!(n.value, Some(AccessValue::Bool(true)));
    }

    #[test]
    fn with_value_float() {
        let n = AccessNode::new("sl", AriaRole::Slider).with_value(
            AccessValue::Float { value: 0.5, min: 0.0, max: 1.0 },
        );
        assert!(matches!(
            n.value,
            Some(AccessValue::Float { value, min, max })
                if (value - 0.5).abs() < f32::EPSILON
                    && (min - 0.0).abs() < f32::EPSILON
                    && (max - 1.0).abs() < f32::EPSILON
        ));
    }

    #[test]
    fn with_value_text_defaults_absent_and_sets() {
        // R739 §5.40 — value_text is a separate additive axis: a plain
        // numeric slider omits it (None), and the labeled variant carries
        // the named-stop string alongside the numeric Float.
        let plain = AccessNode::new("sl", AriaRole::Slider)
            .with_value(AccessValue::Float { value: 0.5, min: 0.0, max: 1.0 });
        assert!(plain.value_text.is_none(), "numeric slider omits aria-valuetext");

        let labeled = AccessNode::new("sl", AriaRole::Slider)
            .with_value(AccessValue::Float { value: 0.5, min: 0.0, max: 1.0 })
            .with_value_text("Medium");
        assert_eq!(labeled.value_text.as_deref(), Some("Medium"));
        // Coexists with the numeric value — the two are complementary.
        assert!(matches!(labeled.value, Some(AccessValue::Float { .. })));
    }

    #[test]
    fn from_interaction_maps_posture_and_leaves_focus_default() {
        use pinion_core::widgets::radio::RadioState;
        // Hover posture -> hovered only; focused stays false (orthogonal).
        let s = AccessState::from_interaction(RadioState::Hover, Some(true));
        assert_eq!(
            s,
            AccessState {
                focused: false,
                disabled: false,
                hovered: true,
                pressed: false,
                checked: Some(true),
            }
        );
        // Disabled posture; checked None.
        let d = AccessState::from_interaction(RadioState::Disabled, None);
        assert!(d.disabled && !d.hovered && !d.pressed && d.checked.is_none());
        // Struct-update syntax overrides focus without disturbing posture.
        let f = AccessState { focused: true, ..AccessState::from_interaction(RadioState::Pressed, None) };
        assert!(f.focused && f.pressed && !f.hovered && !f.disabled);
    }

    #[test]
    fn with_state_replaces_default() {
        let state = AccessState {
            focused: true,
            checked: Some(true),
            ..AccessState::default()
        };
        let n = AccessNode::new("cb", AriaRole::CheckBox).with_state(state);
        assert!(n.state.focused);
        assert_eq!(n.state.checked, Some(true));
        assert!(!n.state.disabled);
    }

    #[test]
    fn with_bounds_sets_rect() {
        let n = AccessNode::new("btn", AriaRole::Button)
            .with_bounds(Rect::new(10, 20, 100, 30));
        assert_eq!(n.bounds, Some(Rect::new(10, 20, 100, 30)));
    }

    #[test]
    fn r863_new_omits_bounds_union_tags() {
        // The frozen-split / tree-grid span axis defaults empty so a
        // single-fragment node resolves bounds from its own tag alone.
        let n = AccessNode::new("row", AriaRole::Row);
        assert!(n.bounds_union_tags.is_empty());
    }

    #[test]
    fn r863_with_bounds_union_tag_appends_in_order() {
        // A frozen-grid Row lists the frozen-pane strip; the substrate later
        // unions its rect into the resolved bounds. Multiple fragments append
        // in call order.
        let n = AccessNode::new("vtbl_row3", AriaRole::Row)
            .with_bounds_union_tag("vtbl_frow3");
        assert_eq!(n.bounds_union_tags, vec!["vtbl_frow3"]);
        let multi = AccessNode::new("tg_drowf1", AriaRole::Row)
            .with_bounds_union_tag("tg#f1")
            .with_bounds_union_tag("tg_xtra");
        assert_eq!(multi.bounds_union_tags, vec!["tg#f1", "tg_xtra"]);
    }

    #[test]
    fn with_child_appends_in_order() {
        let n = AccessNode::new("rg", AriaRole::RadioGroup)
            .with_child("r0")
            .with_child("r1")
            .with_child("r2");
        assert_eq!(n.children, vec!["r0", "r1", "r2"]);
    }

    #[test]
    fn access_state_default_all_false() {
        let s = AccessState::default();
        assert!(!s.focused);
        assert!(!s.disabled);
        assert!(!s.hovered);
        assert!(!s.pressed);
        assert_eq!(s.checked, None);
    }

    // R696 §5.40 — aria-expanded builder + default omission +
    // checked/expanded axis independence. `expanded` is an AccessNode
    // field (mirror `selected` / `modal`), not an AccessState flag.

    #[test]
    fn r696_new_omits_expanded() {
        let n = AccessNode::new("section_hdr", AriaRole::Button);
        assert_eq!(n.expanded, None);
    }

    // R717 §5.40 — aria-autocomplete builder + default omission. The
    // `auto_complete` field defaults absent (aria-autocomplete="none")
    // and is opt-in via `with_auto_complete` (editable combobox only).

    #[test]
    fn r717_new_omits_auto_complete() {
        let n = AccessNode::new("fruit_input", AriaRole::EditableComboBox);
        assert_eq!(n.auto_complete, None);
    }

    #[test]
    fn r717_with_auto_complete_records_axis() {
        let n = AccessNode::new("fruit_input", AriaRole::EditableComboBox)
            .with_auto_complete(AutoComplete::List);
        assert_eq!(n.auto_complete, Some(AutoComplete::List));
    }

    #[test]
    fn r696_with_expanded_records_axis() {
        let open = AccessNode::new("section_hdr", AriaRole::Button).with_expanded(true);
        assert_eq!(open.expanded, Some(true));
        let closed = AccessNode::new("section_hdr", AriaRole::Button).with_expanded(false);
        assert_eq!(closed.expanded, Some(false));
    }

    #[test]
    fn r696_expanded_axis_independent_of_checked() {
        // A disclosure header carries aria-expanded but not
        // aria-checked; the two axes must not alias.
        let n = AccessNode::new("section_hdr", AriaRole::Button).with_expanded(true);
        assert_eq!(n.expanded, Some(true));
        assert_eq!(n.state.checked, None);
    }

    #[test]
    fn r51_98_new_omits_selected_and_multiselectable() {
        let n = AccessNode::new("opt", AriaRole::ListBoxOption);
        assert_eq!(n.selected, None);
        assert!(!n.multiselectable);
    }

    #[test]
    fn r51_98_with_selected_records_axis() {
        let n = AccessNode::new("opt", AriaRole::ListBoxOption).with_selected(true);
        assert_eq!(n.selected, Some(true));
        let n2 = AccessNode::new("opt", AriaRole::ListBoxOption).with_selected(false);
        assert_eq!(n2.selected, Some(false));
    }

    #[test]
    fn r51_98_with_multiselectable_marks_container() {
        let n = AccessNode::new("list", AriaRole::Listbox).with_multiselectable();
        assert!(n.multiselectable);
    }

    // R674 §5.40 — WAI-ARIA hierarchical axes (level / posinset /
    // setsize) builder + default-omission regression tests.

    #[test]
    fn r674_with_level_sets_aria_level() {
        let n = AccessNode::new("row", AriaRole::TreeItem).with_level(1);
        assert_eq!(n.level, Some(1));
        let n2 = AccessNode::new("row", AriaRole::TreeItem).with_level(3);
        assert_eq!(n2.level, Some(3));
    }

    #[test]
    fn r674_with_position_in_set_sets_aria_posinset() {
        let n = AccessNode::new("row", AriaRole::TreeItem)
            .with_position_in_set(2);
        assert_eq!(n.position_in_set, Some(2));
    }

    #[test]
    fn r674_with_size_of_set_sets_aria_setsize() {
        let n = AccessNode::new("row", AriaRole::TreeItem)
            .with_size_of_set(5);
        assert_eq!(n.size_of_set, Some(5));
    }

    // R695 §5.40 — aria-describedby builder + default omission.

    #[test]
    fn r695_new_omits_described_by() {
        let n = AccessNode::new("save_btn", AriaRole::Button);
        assert!(n.described_by.is_none());
    }

    #[test]
    fn r695_with_described_by_records_relation() {
        let n = AccessNode::new("save_btn", AriaRole::Button).with_described_by("save_tip");
        assert_eq!(n.described_by.as_deref(), Some("save_tip"));
    }

    #[test]
    fn r674_hierarchical_axes_compose() {
        // A treeitem typically carries all three axes together —
        // the canonical "item N of M at depth D" announcement
        // requires every value present.
        let n = AccessNode::new("row", AriaRole::TreeItem)
            .with_level(2)
            .with_position_in_set(3)
            .with_size_of_set(7);
        assert_eq!(n.level, Some(2));
        assert_eq!(n.position_in_set, Some(3));
        assert_eq!(n.size_of_set, Some(7));
    }
}
