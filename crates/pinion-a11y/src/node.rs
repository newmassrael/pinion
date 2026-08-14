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

use pinion_core::availability::Unavailable;
use pinion_core::scene::Rect;
use pinion_core::widgets::interaction::InteractionState;
use std::collections::HashMap;

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
    /// R1320 §5.40 §5.27 — name this node from ANOTHER tag's painted label
    /// (the WAI-ARIA `aria-labelledby` relation), resolved by
    /// [`crate::enrich_names_from_scene`] exactly like the self-tag path.
    ///
    /// The forcing consumer is the dock tab well: a `tabpanel` must be named by
    /// its TAB (WAI-ARIA 1.2 §5.3), and after R1318 that tab's painted label is
    /// the panel's DISPLAY title, which is app state the a11y walker has no
    /// access to. Naming the tabpanel from the tab's tag makes the AT tree agree
    /// with the pixels BY CONSTRUCTION — no title has to be threaded into the
    /// a11y layer at all, and any future re-titling follows for free.
    ///
    /// Ignored when [`Self::name`] is already `Some` (an explicit name still
    /// wins, per the enrichment's precedence).
    pub name_from_tag: Option<String>,
    /// Current widget value (boolean for switch/check/radio, float
    /// for slider). Introspect schema reports the same value, by
    /// design (lockstep, single source of truth).
    pub value: Option<AccessValue>,
    /// Interaction-state flags — mirror §5.39 focus + §5.35 hover
    /// + §5.35 pressed.
    pub state: AccessState,
    /// R1668 §5.40 §5.39 — **why** this node is disabled, when the scene's
    /// disabled cascade is what disabled it.
    ///
    /// [`state.disabled`](AccessState::disabled) is the ARIA flag —
    /// `aria-disabled`, and the one bit the reference toolkit's accessibility
    /// layer carries. This is the part that bit cannot hold: a reader is told
    /// that a control is dimmed and never why, so a feature reserved for a
    /// release that has not shipped and one this build will never have are
    /// indistinguishable to somebody using a screen reader.
    ///
    /// Modelled on `aria-describedby` rather than on a state flag, because it
    /// is a description of the node and not a member of the state vocabulary —
    /// which is also why it lives here and not on the `Copy` state struct.
    ///
    /// `None` for a live node, and for a node disabled only by its **own
    /// widget state** (a pressed-out button), whose reason is the widget's
    /// business and not the scene's.
    pub unavailable: Option<Unavailable>,
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
    /// R1523 §5.40 §5.27 — WAI-ARIA `aria-colcount` per WAI-ARIA 1.2 §6.6.4.
    /// The **total** number of columns in a `grid` / `table`, which is not the
    /// number of `columnheader` / `gridcell` children when the column axis is
    /// windowed.
    ///
    /// The column-axis peer of [`Self::size_of_set`]: that says how many rows
    /// the windowed row set is drawn from, this says how many columns the
    /// windowed column set is drawn from. A grid that windows an axis without
    /// declaring that axis' extent is *less* readable than before it scaled —
    /// the AT would report a 200-column table as five columns wide.
    ///
    /// Set on the `grid` container; `None` omits the attribute (a grid whose
    /// every column is present needs no separate extent, though the shared
    /// builders declare it anyway so the two cases read identically).
    pub column_count: Option<u32>,
    /// R1523 §5.40 §5.27 — WAI-ARIA `aria-colindex` per WAI-ARIA 1.2 §6.6.5.
    /// **One-based** absolute column position of this cell / column header
    /// within [`Self::column_count`] columns, so a windowed cell is locatable
    /// even though the columns before it are not in the tree.
    ///
    /// One-based to match the ARIA vocabulary this whole struct is named
    /// after ([`Self::position_in_set`] is one-based for the same reason).
    pub column_index: Option<u32>,
    /// R1560 §5.40 §5.36 — WAI-ARIA `aria-rowcount` per WAI-ARIA 1.2 §6.6.5:
    /// how many rows the table has. The row-axis peer of
    /// [`Self::column_count`], set on the `table` / `grid` container.
    ///
    /// Distinct from [`Self::size_of_set`], which the windowed grid uses to
    /// say how many rows a *set of announced rows* is drawn from. Both exist
    /// because ARIA has both, and a table that is not windowed still has a row
    /// count.
    pub row_count: Option<u32>,
    /// R1560 §5.40 §5.36 — WAI-ARIA `aria-rowindex` per WAI-ARIA 1.2 §6.6.5.
    /// **One-based** absolute row position of this cell or row, the row-axis
    /// mirror of [`Self::column_index`].
    pub row_index: Option<u32>,
    /// R1560 §5.40 §5.36 — WAI-ARIA `aria-rowspan` per WAI-ARIA 1.2 §6.6.5:
    /// how many rows this cell covers. `None` (the default) means one, which
    /// is what the attribute's absence means in ARIA too.
    ///
    /// A cell's span is the half of a table's geometry that a position alone
    /// cannot carry: without it an assistive technology reads a merged cell as
    /// occupying one slot and every cell after it as being in the wrong place.
    /// The toolkit's document tables reach no accessibility interface at all
    /// (accessible text interface has no method that reports block
    /// structure), so this is not a smaller amount of the same thing.
    pub row_span: Option<u32>,
    /// R1560 §5.40 §5.36 — WAI-ARIA `aria-colspan`: how many columns this cell
    /// covers. The column-axis peer of [`Self::row_span`].
    pub column_span: Option<u32>,
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
    /// R1543 §5.40 §5.39 — the platform accelerator that performs this node's
    /// default action (`"Alt+F"`), lowered via
    /// `accesskit::Node::set_access_key`.
    ///
    /// This is HTML's `accesskey` / UIA's `AccessKey` / AT-SPI's key binding — a mnemonic —
    /// and it is deliberately **not** `accesskit::Node::keyboard_shortcut` (UIA `AcceleratorKey`), which names an
    /// application-wide chord such as <kbd>Ctrl</kbd>+S. The two are different
    /// properties and a node may carry both; the toolkit collapses them into
    /// one `Accelerator` string and loses the distinction, so an AT cannot tell a
    /// menu-local mnemonic from a global accelerator.
    ///
    /// Never authored by hand: the §5.40
    /// [`enrich_access_keys_from_scene`](crate::enrich_access_keys_from_scene)
    /// pass derives it from the painted labels, so what an AT announces and
    /// what the shell's <kbd>Alt</kbd> arc dispatches are the same
    /// declaration read twice — the same reason `name` is derived rather than
    /// duplicated into every widget impl.
    ///
    /// Placed on [`AccessNode`] (the R674 / R693 / R695 / R696 / R714 / R717 /
    /// R730 / R731 / R739 additive-axis convention) so it defaults absent
    /// without forcing every hand-written node literal to enumerate it.
    pub access_key: Option<String>,
    /// R1609 §5.40 — WAI-ARIA `aria-live` per WAI-ARIA 1.2 §6.6.10: whether an
    /// assistive technology announces this node's contents changing **without
    /// the user having navigated to it**.
    ///
    /// The channel a change nobody is looking at travels on. Every other axis on
    /// this struct describes a node the user has reached; a live region is how a
    /// consequence *elsewhere* gets said. R1609's forcing consumer is a tile
    /// dashboard's keyboard editing: moving one card pushes others, and the
    /// cards that moved are exactly the ones the user is not on.
    ///
    /// Lowers to `accesskit::Node::set_live`. `None` omits the attribute, which
    /// is what every pre-R1609 node did and what an AT reads as "not live".
    ///
    /// ## Why a declared region and not an announcement event
    ///
    /// The toolkit's peer is accessible announcement event (the toolkit 6.8+,
    /// with a `AnnouncementPoliteness` of `Polite` / `Assertive`), which is **fired** rather than declared — and
    /// measured against the toolkit 6.11.1, *no widget in `the toolkit's widget module/src/widgets` fires one*: the
    /// only references outside `qaccessible.{h,cpp}` are the platform adapters that deliver it
    /// and `qtestaccessible.h`. So the capability is there and the widget set announces
    /// nothing.
    ///
    /// Declaring it is also the only shape compatible with §2 #7. A fired event
    /// leaves no trace, so `scene/access` could not report it and a test could
    /// only observe it by intercepting a callback; a region is part of the scene,
    /// so what an AT will say is derivable from the paint. It also cannot
    /// disagree with itself: two code paths that change the same fact announce
    /// identically because neither of them decides to announce.
    pub live: Option<AccessLive>,
}

/// R1609 §5.40 — how urgently an assistive technology should announce a change
/// inside a live region (WAI-ARIA `aria-live`).
///
/// Three arms where the toolkit's `AnnouncementPoliteness` has two, and the extra one is not
/// padding: an event has no "off" because not firing it *is* off, while a
/// declared region can be nested, so [`Self::Off`] is how a subtree opts out of an
/// ancestor's liveness. Mirrors `accesskit::Live` exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AccessLive {
    /// Changes here are not announced — an explicit opt-out, meaningful inside
    /// an ancestor that is live.
    Off,
    /// Announced when the user is idle, without interrupting. The right default
    /// for a consequence: a card that was pushed out of the way is worth saying
    /// and not worth cutting anyone off for.
    Polite,
    /// Announced immediately, interrupting. For something the user must hear
    /// before continuing.
    Assertive,
}

impl AccessLive {
    /// The WAI-ARIA attribute value.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Polite => "polite",
            Self::Assertive => "assertive",
        }
    }

    /// Lower to the AccessKit vocabulary.
    #[must_use]
    pub const fn to_accesskit(self) -> accesskit::Live {
        match self {
            Self::Off => accesskit::Live::Off,
            Self::Polite => accesskit::Live::Polite,
            Self::Assertive => accesskit::Live::Assertive,
        }
    }
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
            name_from_tag: None,
            value: None,
            state: AccessState::default(),
            unavailable: None,
            bounds: None,
            bounds_union_tags: Vec::new(),
            children: Vec::new(),
            selected: None,
            multiselectable: false,
            level: None,
            position_in_set: None,
            size_of_set: None,
            column_count: None,
            column_index: None,
            row_count: None,
            row_index: None,
            row_span: None,
            column_span: None,
            modal: false,
            described_by: None,
            expanded: None,
            controls: None,
            auto_complete: None,
            sort: None,
            current: None,
            value_text: None,
            has_popup: None,
            access_key: None,
            live: None,
        }
    }

    /// (R1609 §5.40) Declare this node a WAI-ARIA live region — see
    /// [`Self::live`].
    #[must_use]
    pub const fn with_live(mut self, live: AccessLive) -> Self {
        self.live = Some(live);
        self
    }

    /// Set the accessible name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// (R1320 §5.40 §5.27) Name this node from `tag`'s painted label — the
    /// WAI-ARIA `aria-labelledby` relation. See [`Self::name_from_tag`].
    #[must_use]
    pub fn with_name_from_tag(mut self, tag: impl Into<String>) -> Self {
        self.name_from_tag = Some(tag.into());
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

    /// R1229 §5.40 — mark this checkbox as WAI-ARIA `aria-checked="mixed"` (the
    /// HTML `<input>.indeterminate` axis, [`AccessState::mixed`]): an
    /// indeterminate multi-object checkbox with no definite on/off. Lowers to
    /// accesskit `Toggled::Mixed`, taking precedence over any `checked` /
    /// [`AccessValue::Bool`]. Use on a `CheckBox` / `MenuItemCheckbox` only.
    #[must_use]
    pub fn with_mixed(mut self) -> Self {
        self.state.mixed = true;
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

    /// R1523 §5.40 — set the WAI-ARIA `aria-colcount` attribute. See
    /// [`Self::column_count`] for the semantic axis.
    #[must_use]
    pub fn with_column_count(mut self, columns: u32) -> Self {
        self.column_count = Some(columns);
        self
    }

    /// R1523 §5.40 — set the WAI-ARIA `aria-colindex` attribute from a
    /// **zero-based** column index: the stored value is the one-based
    /// `col + 1`, mirroring [`Self::with_set_position`]'s handling of
    /// `aria-posinset` so no caller has to remember which axis is off by one.
    #[must_use]
    pub fn with_column(mut self, col: usize) -> Self {
        self.column_index = Some(u32::try_from(col + 1).unwrap_or(u32::MAX));
        self
    }

    /// R1560 §5.40 — set the WAI-ARIA `aria-rowcount` attribute. See
    /// [`Self::row_count`] for the semantic axis.
    #[must_use]
    pub fn with_row_count(mut self, rows: u32) -> Self {
        self.row_count = Some(rows);
        self
    }

    /// R1560 §5.40 — set `aria-rowindex` from a **zero-based** row index: the
    /// stored value is the one-based `row + 1`, exactly as
    /// [`Self::with_column`] handles the other axis, so no caller has to
    /// remember which of the two is off by one.
    #[must_use]
    pub fn with_row(mut self, row: usize) -> Self {
        self.row_index = Some(u32::try_from(row + 1).unwrap_or(u32::MAX));
        self
    }

    /// R1560 §5.40 — set `aria-rowspan` / `aria-colspan` from the extent a
    /// cell covers.
    ///
    /// One builder for the pair because they are one fact — the rectangle the
    /// cell occupies — and a caller that could state half of it would produce
    /// a cell whose announced shape is not the one it was allocated.
    #[must_use]
    pub fn with_span(mut self, rows: u32, columns: u32) -> Self {
        self.row_span = Some(rows);
        self.column_span = Some(columns);
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

    /// R1544 §5.40 — set the WAI-ARIA `aria-readonly` axis: the node's value
    /// is presented but not editable. See [`AccessState::read_only`].
    #[must_use]
    pub fn with_read_only(mut self, read_only: bool) -> Self {
        self.state.read_only = read_only;
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
    /// R1229 §5.40 — WAI-ARIA `aria-checked="mixed"`: the *indeterminate* leg of
    /// a tri-state checkbox, the HTML `<input type=checkbox>.indeterminate`
    /// property — a **separate** axis from [`checked`](Self::checked) exactly as
    /// in the DOM. `true` marks a multi-object checkbox whose members disagree
    /// (no definite on/off); it lowers to accesskit `Toggled::Mixed`, taking
    /// precedence over `checked` / [`AccessValue::Bool`]. Only a `CheckBox` /
    /// `MenuItemCheckbox` may be mixed — a `Switch` / `RadioButton` is
    /// two-state (the WAI-ARIA `aria-checked` value table). Default `false`.
    pub mixed: bool,
    /// R1544 §5.40 — WAI-ARIA `aria-readonly`: the value is **presented but
    /// not editable**, the axis a Model/View grid's `ItemIsEditable` flag
    /// controls per cell.
    ///
    /// A separate axis from [`disabled`](Self::disabled), exactly as in
    /// WAI-ARIA: a disabled control is inert (not focusable, not perceivable
    /// as actionable), a read-only one is fully focusable and copyable and
    /// simply refuses to change. A grid that marked its fixed columns
    /// `disabled` would make them unreachable by AT navigation, which is a
    /// different — and wrong — statement.
    ///
    /// Lowers to accesskit `set_read_only`. Default `false`, so nothing that
    /// does not opt in changes: a node that says nothing about editability is
    /// how every pre-R1544 node behaved, and "unspecified" is what an AT
    /// assumes when the property is absent.
    pub read_only: bool,
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
            // R1229 — the indeterminate axis is a distinct opt-in (`with_mixed`),
            // not derived from the interaction enum; interaction never implies mixed.
            mixed: false,
            // R1544 — editability is the *model's* answer, never the
            // interaction enum's: a hovered cell and a read-only cell are
            // orthogonal facts. Consumers set it through
            // `AccessNode::with_read_only`.
            read_only: false,
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

/// R1560 §5.40 — an index of `nodes` by tag, so a pass that describes many
/// objects can find each one without scanning.
///
/// The lifted half of the find-or-create every scene-derived pass does
/// ([`attach_block_headings`](crate::attach_block_headings), the two halves of
/// [`attach_block_lists`](crate::attach_block_lists), and
/// [`attach_block_tables`](crate::attach_block_tables) — four sites, so this
/// is a lift rather than a speculation). Each of them found its node with a
/// linear `find` per object, which is quadratic in the objects a pass
/// describes: tolerable for a document's headings, which are few, and not for
/// a table's cells, which are not.
#[derive(Debug, Default)]
pub struct NodeIndex {
    by_tag: HashMap<String, usize>,
}

impl NodeIndex {
    /// Index the nodes a binding has already described.
    #[must_use]
    pub fn new(nodes: &[AccessNode]) -> Self {
        Self {
            by_tag: nodes
                .iter()
                .enumerate()
                .map(|(at, node)| (node.tag.clone(), at))
                .collect(),
        }
    }

    /// The node for `tag`, appending one with `role` if the binding did not
    /// already describe it.
    ///
    /// The caller keeps its own merge policy — which fields survive an
    /// existing description is a decision per pass (a heading outranks a list
    /// item), and folding that in here would make one rule serve four
    /// questions.
    pub fn upsert<'n>(
        &mut self,
        nodes: &'n mut Vec<AccessNode>,
        tag: &str,
        role: AriaRole,
    ) -> &'n mut AccessNode {
        let at = *self.by_tag.entry(tag.to_owned()).or_insert_with(|| {
            nodes.push(AccessNode::new(tag.to_owned(), role));
            nodes.len() - 1
        });
        &mut nodes[at]
    }
}

/// R1691 §5.40 — every tag some node in `nodes` **points at**.
///
/// A node can name another: as a composite child, as a rectangle contributor,
/// as the source of its own name, as the description read when a reader lands
/// on it, or as the thing it controls. A tag in this set is reachable through
/// whoever names it, whatever paints — which is exactly what separates a
/// deliberately **virtual** node (the form painter's description regions have
/// no rectangle of their own and are announced anyway) from a name a reader can
/// be sent to and never find.
///
/// Lifted because it was computed twice — once in the `scene/voice` handler and
/// once in the screen's own gate — and the two must not disagree about what
/// counts as a reference. It reads **every** reference-carrying field, so a new
/// kind of reference cannot silently start producing false alarms; that is the
/// error direction to prefer, since a false alarm is noticed and a miss is not.
/// R1692 §5.40 — what the tree says about each tag, as the voice census reads it.
///
/// The census needs two things a list of tags cannot give it: **the name a
/// reader hears** — absent, an address, or unpronounceable are three different
/// defects and none is visible from the tag — and **what each node is composed
/// of**, which is what anchors a composite container whose own tag the scene
/// never paints (a legend, a transcript, a radio group).
///
/// [`children`](AccessNode::children) and
/// [`bounds_union_tags`](AccessNode::bounds_union_tags) are the composing
/// fields, and the other three reference-carrying fields deliberately are not:
/// a description, a name source and a controlled target are all statements
/// about *another* node's content, and say nothing about whether this one has
/// anything behind it.
///
/// Lifted here for the reason [`referenced_tags`] was: the wire handler and
/// every screen's own gate must not each decide what an announcement is.
#[must_use]
pub fn announcements(
    nodes: &[AccessNode],
) -> std::collections::BTreeMap<String, pinion_core::voice::Announcement> {
    nodes
        .iter()
        .map(|node| {
            let mut composes = node.children.clone();
            composes.extend(node.bounds_union_tags.iter().cloned());
            (
                node.tag.clone(),
                pinion_core::voice::Announcement {
                    name: node.name.clone().unwrap_or_default(),
                    // The one judgment the census cannot make for itself: it
                    // does not know what a `cell` is.
                    name_required: node.role.name_required(),
                    // A live region is announced when it changes rather than
                    // navigated to, so it owes no rectangle — the standard
                    // WAI-ARIA shape for reporting a filtered count or a panel
                    // that rearranged.
                    live: node.live.is_some(),
                    composes,
                },
            )
        })
        .collect()
}

#[must_use]
pub fn referenced_tags(nodes: &[AccessNode]) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for node in nodes {
        out.extend(node.children.iter().cloned());
        out.extend(node.bounds_union_tags.iter().cloned());
        for tag in [&node.described_by, &node.name_from_tag, &node.controls]
            .into_iter()
            .flatten()
        {
            out.insert(tag.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ★★★★ R1692 — what the census reads out of a node, field by field. A
    /// counterfactual asked for this: making `announcements` report **no node as
    /// a live region** — which turns every off-screen status region back into a
    /// `ghost` across the tree — compiled and left every crate test green,
    /// because nothing here tested this function at all.
    #[test]
    fn r1692_an_announcement_carries_what_the_census_cannot_see() {
        let nodes = vec![
            AccessNode::new("legend", AriaRole::Group)
                .with_name("Series".to_owned())
                .with_child("legend#0")
                .with_bounds_union_tag("legend.strip")
                // Statements about ANOTHER node's content: they say nothing
                // about whether this one has anything behind it.
                .with_described_by("said")
                .with_name_from_tag("titled")
                .with_controls("driven"),
            AccessNode::new("blank", AriaRole::Cell),
            AccessNode::new("count", AriaRole::Status)
                .with_name("19 properties".to_owned())
                .with_live(AccessLive::Polite),
        ];
        let out = announcements(&nodes);
        assert_eq!(out.len(), 3);

        let legend = &out["legend"];
        assert_eq!(legend.name, "Series");
        assert!(legend.name_required, "a group is named or it is nothing");
        assert!(!legend.live);
        assert_eq!(
            legend.composes,
            ["legend#0", "legend.strip"],
            "composition is membership and extent, not every reference",
        );

        let blank = &out["blank"];
        assert!(blank.name.is_empty());
        assert!(
            !blank.name_required,
            "an empty cell is what the data says, not an omission",
        );

        let count = &out["count"];
        assert!(count.live, "a live region owes no rectangle");
        assert!(count.composes.is_empty());
    }

    /// R1691 — every field a node can point through is read, and a node nobody
    /// points at is absent. The second half is what the census asks.
    #[test]
    fn r1691_referenced_tags_reads_every_pointing_field() {
        let nodes = vec![
            AccessNode::new("a", AriaRole::Button)
                .with_child("child")
                .with_bounds_union_tag("union")
                .with_described_by("said")
                .with_name_from_tag("named")
                .with_controls("driven"),
            AccessNode::new("lonely", AriaRole::Button),
        ];
        let seen = referenced_tags(&nodes);
        for tag in ["child", "union", "said", "named", "driven"] {
            assert!(seen.contains(tag), "{tag} is pointed at and was not seen");
        }
        assert!(
            !seen.contains("lonely"),
            "a node nobody points at is not referenced by existing",
        );
        assert!(
            !seen.contains("a"),
            "and neither is the one doing the pointing"
        );
    }

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
        attach_child_button(
            &mut nodes,
            "cell",
            "cell#reset".to_owned(),
            "Reset".to_owned(),
        );
        // The host now references the button, and the button node exists.
        let host = nodes
            .iter()
            .find(|n| n.tag == "cell")
            .expect("host present");
        assert!(
            host.children.contains(&"cell#reset".to_owned()),
            "host links the button"
        );
        let btn = nodes
            .iter()
            .find(|n| n.tag == "cell#reset")
            .expect("button emitted");
        assert_eq!(btn.role, AriaRole::Button);
        assert_eq!(btn.name.as_deref(), Some("Reset"));
    }

    #[test]
    fn attach_child_button_emits_nothing_for_an_absent_host() {
        // R984.1 — orphan-free by construction: an affordance whose host is not
        // in the tree (windowed out) must leave NO dangling button node behind.
        let mut nodes = vec![AccessNode::new("other", AriaRole::GridCell)];
        attach_child_button(
            &mut nodes,
            "absent",
            "absent#reset".to_owned(),
            "Reset".to_owned(),
        );
        assert_eq!(
            nodes.len(),
            1,
            "no button node is pushed when the host is absent"
        );
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
        let n = AccessNode::new("cb", AriaRole::CheckBox).with_value(AccessValue::Bool(true));
        assert_eq!(n.value, Some(AccessValue::Bool(true)));
    }

    #[test]
    fn with_value_float() {
        let n = AccessNode::new("sl", AriaRole::Slider).with_value(AccessValue::Float {
            value: 0.5,
            min: 0.0,
            max: 1.0,
        });
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
        let plain = AccessNode::new("sl", AriaRole::Slider).with_value(AccessValue::Float {
            value: 0.5,
            min: 0.0,
            max: 1.0,
        });
        assert!(
            plain.value_text.is_none(),
            "numeric slider omits aria-valuetext"
        );

        let labeled = AccessNode::new("sl", AriaRole::Slider)
            .with_value(AccessValue::Float {
                value: 0.5,
                min: 0.0,
                max: 1.0,
            })
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
                hovered: true,
                checked: Some(true),
                ..AccessState::default()
            }
        );
        // Disabled posture; checked None.
        let d = AccessState::from_interaction(RadioState::Disabled, None);
        assert!(d.disabled && !d.hovered && !d.pressed && d.checked.is_none());
        // Struct-update syntax overrides focus without disturbing posture.
        let f = AccessState {
            focused: true,
            ..AccessState::from_interaction(RadioState::Pressed, None)
        };
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
        let n = AccessNode::new("btn", AriaRole::Button).with_bounds(Rect::new(10, 20, 100, 30));
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
        let n = AccessNode::new("vtbl_row3", AriaRole::Row).with_bounds_union_tag("vtbl_frow3");
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
        let n = AccessNode::new("row", AriaRole::TreeItem).with_position_in_set(2);
        assert_eq!(n.position_in_set, Some(2));
    }

    #[test]
    fn r674_with_size_of_set_sets_aria_setsize() {
        let n = AccessNode::new("row", AriaRole::TreeItem).with_size_of_set(5);
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
