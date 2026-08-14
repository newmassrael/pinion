//! R51.61 §5.40 — Pinion ARIA role enum.
//!
//! [`AriaRole`] is a pinion-native subset of `accesskit::Role` that
//! covers the framework's standard widget catalogue (Button / Switch /
//! `CheckBox` / `RadioButton` / `Slider` / `RadioGroup`) plus a `Generic`
//! fallback. The wrapper keeps the public `WidgetView::access_node`
//! return type stable across `accesskit` minor-version bumps — a future
//! `accesskit` upgrade rewrites only `to_accesskit` here, not every
//! widget impl.
//!
//! ARIA role names follow the WAI-ARIA 1.2 specification literal
//! spelling so introspect / RPC consumers see the same identifiers
//! they would in HTML's `role` attribute.

use accesskit::{
    AriaCurrent as AkAriaCurrent, AutoComplete as AkAutoComplete, HasPopup as AkHasPopup, Role,
    SortDirection as AkSortDirection,
};

/// Pinion-native ARIA role enum.
///
/// One-to-one mapping with `accesskit::Role` for the widgets pinion
/// ships in its standard catalogue. The `Generic` variant maps to
/// `accesskit::Role::GenericContainer` and is the default for any
/// scene element that opts into the a11y tree without claiming a
/// specific role.
///
/// `#[non_exhaustive]` is deliberate — additional widgets (`TextInput`,
/// `ScrollBar`, etc.) land additively as new axes ratify.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AriaRole {
    /// WAI-ARIA 1.2 §4.3 `button` role: a clickable control that
    /// triggers an action (the default `ButtonExternal` / disclosure
    /// header / segment).
    ///
    /// **`aria-pressed` (toggle button) vs `aria-checked`** (R733
    /// §5.40): a `button` that additionally carries
    /// [`AccessState::checked`](crate::node::AccessState::checked) =
    /// `Some(_)` is a **toggle button** — AT announces it as
    /// "pressed" / "not pressed". This is distinct from a
    /// [`Self::CheckBox`] / [`Self::Switch`] / [`Self::RadioButton`]
    /// carrying the same `checked`, which AT announces as "checked".
    /// Both lower through the *same* AccessKit `set_toggled` call
    /// ([`crate::tree`] `lower_access_node`); the `aria-pressed` vs
    /// `aria-checked` split is purely a function of the role, exactly
    /// as the WAI-ARIA spec defines (a button reflects `aria-pressed`,
    /// not `aria-checked`). The multi-select segmented button
    /// (`hello-segmented-multi`, R733) is the first toggle-button
    /// consumer: N independent `button[aria-pressed]` segments under a
    /// [`Self::Group`] parent. A `button` with `checked = None` is a
    /// plain (non-toggle) button — the accordion header (R697) carries
    /// `aria-expanded` instead, with `checked = None`.
    Button,
    Switch,
    CheckBox,
    RadioButton,
    Slider,
    RadioGroup,
    /// R51.96.1 §5.40 — WAI-ARIA 1.2 §4.3.x `listbox` composite
    /// role. Container for [`Self::ListBoxOption`] children; pairs
    /// with the `ListBox` composite primitive
    /// ([`pinion_core::widgets::listbox`]). Distinct from
    /// [`Self::RadioGroup`] in the WAI-ARIA keyboard model: Arrow
    /// keys move focus only, `Space` / `Enter` commits the
    /// selection (vs Arrow-activates-immediately for `RadioGroup`).
    Listbox,
    /// R51.96.1 §5.40 — WAI-ARIA 1.2 `option` role. Single
    /// selectable child of an [`Self::Listbox`] parent. Distinct
    /// from [`Self::RadioButton`] only at the AT-side role surface;
    /// the underlying widget primitive
    /// ([`pinion_core::widgets::listbox_item`]) shares the
    /// button-like statechart with `Radio` / `Toggle` / `Checkbox`.
    ListBoxOption,
    /// R714 §5.40 — WAI-ARIA 1.2 §4.5 `combobox` role: the focusable
    /// control that displays the current value and opens a popup
    /// ([`Self::Listbox`] of [`Self::ListBoxOption`]s) for selection.
    /// Pairs with the `hello-combobox` composition (a `ButtonExternal`
    /// trigger + `ListBoxExternal` popup + transparent-scrim
    /// light-dismiss barrier). The collapsed-vs-open state is carried by
    /// [`crate::AccessNode::expanded`] (`aria-expanded`); the controlled
    /// popup by [`crate::AccessNode::controls`] (`aria-controls`); the
    /// active option by `aria-activedescendant` (the composite
    /// `access_focus_target`). Distinct from [`Self::Listbox`]: the
    /// combobox is the *trigger* surface, the listbox is the *popup*.
    /// This first slice is the WAI-ARIA "select-only" combobox (the
    /// value is chosen from the list, not typed); an editable
    /// combobox would lower to `Role::EditableComboBox` additively.
    ComboBox,
    /// R717 §5.40 — WAI-ARIA 1.2 §4.5 *editable* combobox: a
    /// single-line text input that owns a popup [`Self::Listbox`]
    /// (the "Editable Combobox With List Autocomplete" APG pattern).
    /// The user types into the trigger; the popup filters to matching
    /// options (`aria-autocomplete="list"`, carried by
    /// [`crate::AccessNode::auto_complete`]). The collapsed-vs-open
    /// state is [`crate::AccessNode::expanded`] (`aria-expanded`); the
    /// controlled popup [`crate::AccessNode::controls`]
    /// (`aria-controls`); the highlighted (not focused) option
    /// `aria-activedescendant` while focus stays in the input.
    ///
    /// Distinct from [`Self::ComboBox`] (select-only) only at the
    /// AccessKit layer: AccessKit splits the WAI-ARIA `combobox` role
    /// across `Role::ComboBox` (select-only) and
    /// `Role::EditableComboBox` (typed-value) for the platform AX
    /// mapping, so [`Self::to_accesskit`] differs while
    /// [`Self::aria_name`] stays `"combobox"` for both (WAI-ARIA 1.2
    /// has a single `combobox` role; editability is conveyed by the
    /// input + `aria-autocomplete`, not a separate role token).
    EditableComboBox,
    /// R56.1.b.1 §5.40 — WAI-ARIA 1.2 §4.3 `textbox` role
    /// (single-line input). Pairs with the §5.38 `TextField` widget
    /// primitive ([`pinion_core::widgets::text_field`]). The role
    /// name lowers to `accesskit::Role::TextInput` — AccessKit splits
    /// the WAI-ARIA `textbox` role across two enum members
    /// (`TextInput` for single-line, `MultilineTextInput` for the
    /// `aria-multiline=true` variant); pinion's single-line first
    /// slice maps to `TextInput`. A future multiline `TextArea` axis
    /// adds the second variant additively.
    TextInput,
    /// R656 §5.40 — WAI-ARIA 1.2 §5.3.5 `list` role. Container for
    /// [`Self::ListItem`] children carrying a flat sequence of items.
    /// Distinct from [`Self::Listbox`]: a `list` is a passive AT
    /// container with no keyboard model (Tab traversal lands on item
    /// children, not the list root), while a `listbox` owns the
    /// roving-tabindex + Arrow/Space activation model. Pinion's
    /// `todomvc` composed app uses `List` for the user-driven todo
    /// collection — items are added by Enter on a separate text field
    /// (not Arrow-key navigation), and the list itself never receives
    /// focus, matching the WAI-ARIA `list` semantics exactly.
    List,
    /// R656 §5.40 — WAI-ARIA 1.2 §5.3.6 `listitem` role. Single child
    /// of a [`Self::List`] parent. AT presents each item as a
    /// distinct entry with its own name + (optionally) interactive
    /// descendants; AT tools (screen readers, `VoiceOver`, Orca)
    /// announce position-in-list ("item 2 of 3") automatically when
    /// items are nested under a `List` parent. Distinct from
    /// [`Self::ListBoxOption`]: a `listitem` does not participate in
    /// a selection model (no `aria-selected`), while a `listboxoption`
    /// always carries one.
    ListItem,
    /// R673 §5.40 — WAI-ARIA 1.2 §5.3.10 `tree` role. Hierarchical
    /// container of [`Self::TreeItem`] children. Distinct from
    /// [`Self::List`]: a `tree` owns the keyboard model (Arrow
    /// Up/Down navigates, Arrow Right expands, Arrow Left
    /// collapses, Home/End jump to first/last visible row); a
    /// `list` is a passive AT container. Pairs with the §5.50
    /// `pinion_widget_paint::tree_view` substrate (R671) +
    /// the interactive consumer pattern (R673 `hello-tree-view`).
    Tree,
    /// R673 §5.40 — WAI-ARIA 1.2 §5.3.11 `treeitem` role. Single
    /// child of a [`Self::Tree`] parent. Each row in the flat row
    /// sequence the `tree_view` substrate emits carries this role.
    ///
    /// **Authoring requirement** (R674 §5.40 correction): per
    /// WAI-ARIA 1.2 §6.6.8 / §6.6.9 / §6.6.10, `aria-level` /
    /// `aria-posinset` / `aria-setsize` are *required* on custom
    /// widget roles — AT does **not** infer them from DOM nesting
    /// for `role="treeitem"`. Pinion paint scenes are flat row
    /// sequences (the substrate stamps composite tags per row), so
    /// the binding's `access_node` walker is the sole source of
    /// truth for these values. Provide them via
    /// [`AccessNode::with_level`] / [`AccessNode::with_position_in_set`] /
    /// [`AccessNode::with_size_of_set`] when emitting per-row
    /// descriptors. `aria-expanded` is carried by [`AccessState`] on
    /// future axes; today branches encode the expanded glyph in the
    /// paint scene rather than the a11y state.
    ///
    /// [`AccessNode::with_level`]: crate::AccessNode::with_level
    /// [`AccessNode::with_position_in_set`]: crate::AccessNode::with_position_in_set
    /// [`AccessNode::with_size_of_set`]: crate::AccessNode::with_size_of_set
    /// [`AccessState`]: crate::node::AccessState
    TreeItem,
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 `tablist` role. Container for
    /// [`Self::Tab`] children that owns the roving-tabindex keyboard
    /// model (Arrow Left/Right move between tabs, Home/End jump to
    /// first/last). Distinct from [`Self::RadioGroup`] only at the AT
    /// role surface: the underlying selection substrate is shared
    /// ([`pinion_core::widgets::radio_group::RadioGroupExternal`] —
    /// "select 1 of N" is identical semantics), but a `tablist`
    /// announces "tab list" and its children announce "tab" rather
    /// than "radio button". Pairs with the §5.50
    /// `pinion_widget_paint::tabs` substrate (R690) + the
    /// `hello-tabs` consumer.
    TabList,
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 `tab` role. Single selectable
    /// child of a [`Self::TabList`] parent. Carries `aria-selected`
    /// (via [`AccessNode::with_selected`]) for the active tab and
    /// the WAI-ARIA 1.2 §6.6.9 / §6.6.10 `aria-posinset` /
    /// `aria-setsize` sibling axes ("tab N of M"). Distinct from
    /// [`Self::RadioButton`]: a tab is *selected*, not *checked* —
    /// the truthy axis is `aria-selected`, matching
    /// [`Self::ListBoxOption`].
    ///
    /// `aria-controls` (the tab → tab-panel relationship) is a future
    /// additive axis once a consumer surfaces multi-panel addressing;
    /// R690 renders a single panel for the active tab, so the
    /// relationship is structurally implicit.
    ///
    /// [`AccessNode::with_selected`]: crate::AccessNode::with_selected
    Tab,
    /// R690 §5.40 — WAI-ARIA 1.2 §3.6 `tabpanel` role. The content
    /// region associated with the active [`Self::Tab`]. Focusable
    /// (Tab key lands on the panel when it has no focusable content)
    /// so it shares the `Focus`-only action set with the other
    /// container roles. Only the active tab's panel is rendered, so
    /// at most one `tabpanel` node exists in the tree at a time.
    TabPanel,
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 `menubar` role. A horizontal
    /// presentation of a [`Self::Menu`] that usually stays visible
    /// (the editor `File` / `Edit` / `View` bar). Container for
    /// [`Self::MenuItem`] title children; owns the WAI-ARIA §3.5
    /// menubar keyboard model (Arrow Left/Right move between
    /// top-level items, Arrow Down opens the focused item's menu).
    /// Joins the focus-only container action set alongside
    /// [`Self::TabList`] / [`Self::Tree`] / [`Self::Listbox`].
    MenuBar,
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 `menu` role. The dropdown
    /// container shown when a [`Self::MenuBar`] title opens. Holds the
    /// open menu's [`Self::MenuItem`] children and owns the in-menu
    /// keyboard model (Arrow Up/Down move the active item, Home/End
    /// jump, Escape closes). Focus-only container action set — the
    /// active item is reported as the menu's `aria-activedescendant`,
    /// not as a focus of the container itself.
    Menu,
    /// R691 §5.40 — WAI-ARIA 1.2 §3.5 `menuitem` role. A single
    /// **command** within a [`Self::MenuBar`] (a top-level title) or
    /// a [`Self::Menu`] (a dropdown command). Commit-class atomic at
    /// the AT-action surface (Click activates, Focus moves the AT
    /// cursor) — the action set matches [`Self::Button`] /
    /// [`Self::Tab`].
    ///
    /// **Distinct from selection roles.** A base `menuitem` carries
    /// neither `aria-selected` ([`Self::Tab`] / [`Self::ListBoxOption`])
    /// nor `aria-checked` ([`Self::RadioButton`] / [`Self::CheckBox`]):
    /// activating it fires a one-shot command and (for a dropdown
    /// item) closes the menu. The stateful menu variants
    /// (`menuitemcheckbox` / `menuitemradio`) are separate WAI-ARIA
    /// roles that land additively if a consumer needs a toggled menu
    /// entry. This command-vs-selection split is why the §5.50 menu
    /// substrate (R691) uses a command-class `MenuBarExternal` rather
    /// than reusing the
    /// [`RadioGroupExternal`](pinion_core::widgets::radio_group::RadioGroupExternal)
    /// the way [`Self::Tab`] does.
    ///
    /// `aria-haspopup` (a top-level title that opens a submenu) and
    /// `aria-expanded` (the open/closed state of that submenu) are
    /// future additive `AccessNode` axes; R691 encodes the open menu
    /// structurally (the dropdown `Menu` node is present only while
    /// open) rather than on the title's a11y state.
    MenuItem,
    /// R805 §5.40 — WAI-ARIA 1.2 `menuitemcheckbox`: a dropdown item that
    /// toggles a persistent boolean (View > Show Grid). Unlike
    /// [`Self::MenuItem`] it carries `aria-checked`
    /// ([`AccessState::checked`](crate::AccessState::checked)); activating
    /// it flips the state and (like a command) closes the menu. The sibling
    /// `menuitemradio` role lands additively when the menu radio-group
    /// consumer arrives (R806).
    MenuItemCheckbox,
    /// R692 §5.40 — WAI-ARIA 1.2 §3.4 `toolbar` role. A horizontal
    /// grouping of controls (the editor format / command strip every
    /// DCC / IDE / CAD tool ships) that owns the roving-tabindex
    /// keyboard model: the toolbar is a single Tab stop, Arrow
    /// Left/Right move the roving focus between controls, Home/End
    /// jump to first/last. Joins the focus-only container action set
    /// alongside [`Self::MenuBar`] / [`Self::TabList`] / [`Self::Tree`].
    ///
    /// Distinct from [`Self::MenuBar`]: a `toolbar`'s children are
    /// directly-actionable controls (command [`Self::Button`]s +
    /// toggle buttons carrying `aria-pressed`), not titles that open
    /// dropdown [`Self::Menu`]s. A toggle button is a `Button` whose
    /// [`AccessState::checked`](crate::node::AccessState) lowers to the
    /// AccessKit `Toggled` attribute — the WAI-ARIA `aria-pressed`
    /// axis emerges from the `button` role + the toggled state (vs the
    /// `aria-checked` axis a `checkbox` role carries for the same
    /// underlying attribute).
    Toolbar,
    /// R693 §5.40 — WAI-ARIA 1.2 §3.2 `dialog` role. A modal container
    /// (the confirm / form / alert dialog every pro tool ships) that
    /// holds the focus trap while open: focus moves into the dialog on
    /// open, Tab is confined to the dialog's controls, Escape dismisses,
    /// and focus returns to the invoker on close (the
    /// [`crate::node::AccessNode::modal`] flag lowers to the AccessKit
    /// `aria-modal` attribute so AT announces the modality).
    ///
    /// Focus-only container action set (alongside [`Self::MenuBar`] /
    /// [`Self::Toolbar`] / [`Self::Tree`]): the dialog itself is a
    /// grouping node; its action buttons are child [`Self::Button`]s.
    Dialog,
    /// R695 §5.40 — WAI-ARIA 1.2 §3.7 `tooltip` role. A contextual
    /// descriptive popup shown on hover / keyboard focus of a trigger
    /// element (WCAG 2.2 SC 1.4.13 "Content on Hover or Focus"). The
    /// tooltip carries **no interactive semantics** — it is a passive
    /// label region the trigger references through
    /// [`AccessNode::with_described_by`](crate::AccessNode::with_described_by)
    /// (the `aria-describedby` relation), so AT announces the tooltip
    /// text as the trigger's description rather than as a focusable
    /// node. Distinct from [`Self::Dialog`] (a focus container) and
    /// [`Self::Menu`] (a command popup): a tooltip never receives focus
    /// and owns no keyboard model of its own — `Escape` dismisses it
    /// while focus stays on the trigger.
    Tooltip,
    /// R704 §5.40 — WAI-ARIA 1.2 §3.3 `grid` role. An interactive
    /// tabular container with a two-dimensional navigation model
    /// (Arrow keys move between cells, the container owns a single Tab
    /// stop). Pinion's first consumer is the §5.50
    /// `pinion_widget_paint::datepicker` month calendar (R704
    /// `hello-datepicker`), whose [`Self::GridCell`] children are the
    /// day cells. Distinct from [`Self::Listbox`]: a `grid` navigates in
    /// two dimensions (row × column) and its cells carry
    /// `aria-posinset` / `aria-setsize`, while a `listbox` is a
    /// one-dimensional option list. A future `Table` axis (non-
    /// interactive tabular data) would land additively — WAI-ARIA
    /// distinguishes `grid` (interactive widget) from `table` (static
    /// data); AccessKit carries both
    /// ([`accesskit::Role::Grid`] vs `Role::Table`), so the split is
    /// preserved end to end. R1560 landed that axis: see [`Self::Table`].
    Grid,
    /// R1560 §5.40 §5.36 — WAI-ARIA 1.2 §3.3 `table` role. **Static**
    /// tabular content: a document's table
    /// ([`pinion_core::text_table`]), where the cells
    /// are read rather than operated.
    ///
    /// The axis [`Self::Grid`]'s doc predicted, landed additively and kept
    /// separate for the reason that doc gives: a `grid` is an interactive
    /// widget with a two-dimensional keyboard model and a single Tab stop,
    /// while a `table` owns no keyboard model at all. Announcing a document
    /// table as a `grid` would promise a navigation the reader does not have.
    /// Its rows are [`Self::Row`] and its cells [`Self::Cell`], and it carries
    /// `aria-rowcount` / `aria-colcount`.
    Table,
    /// R1560 §5.40 §5.36 — WAI-ARIA 1.2 §3.3 `cell` role. A single cell of a
    /// [`Self::Table`], carrying `aria-rowindex` / `aria-colindex` and, when
    /// it covers more than one slot, `aria-rowspan` / `aria-colspan`.
    ///
    /// Distinct from [`Self::GridCell`] exactly as `table` is from `grid`: a
    /// `gridcell` is activatable and selectable, a `cell` is content.
    Cell,
    /// R704 §5.40 — WAI-ARIA 1.2 §3.3 `gridcell` role. A single cell of
    /// a [`Self::Grid`] (one selectable day in the date picker).
    /// Commit-class atomic at the AT-action surface (Click activates,
    /// Focus moves the AT cursor) — the action set matches
    /// [`Self::Button`] / [`Self::ListBoxOption`]. Carries
    /// `aria-selected` (via [`AccessNode::with_selected`]) for the
    /// chosen day, plus the WAI-ARIA 1.2 §6.6.9 / §6.6.10
    /// `aria-posinset` / `aria-setsize` axes ("day N of M"). The truthy
    /// axis is `aria-selected` (the cell is *selected*, not *checked* —
    /// matching [`Self::ListBoxOption`] / [`Self::Tab`]).
    ///
    /// [`AccessNode::with_selected`]: crate::AccessNode::with_selected
    GridCell,
    /// R704 §5.40 — WAI-ARIA 1.2 §3.3 `columnheader` role. A header cell
    /// labelling a [`Self::Grid`] column (the weekday headers Su..Sa in
    /// the date picker). Structural / non-interactive: AT announces the
    /// header name when reading a cell in that column, but the header
    /// itself owns no keyboard model and no AT actions (matching
    /// [`Self::Tooltip`]'s passive arm).
    ColumnHeader,
    /// R863 §5.40 §5.27 — WAI-ARIA 1.2 §3.3 `rowheader` role. A header cell
    /// labelling a [`Self::Row`] — the row analogue of [`Self::ColumnHeader`].
    /// Pinion's first consumer is the tree-grid's frozen **name** column: the
    /// `{tag}#{id}` name cell is the row's label, so it lowers to `rowheader`
    /// (the metadata columns lower to plain [`Self::GridCell`]). Structural /
    /// non-interactive at the AT-action surface, matching [`Self::ColumnHeader`]
    /// — the tree-grid routes a *click* on the name cell to the toggle/select
    /// coordinator, but the AT role itself owns no keyboard model.
    RowHeader,
    /// R704 §5.40 — WAI-ARIA 1.2 §3.3 `row` role. A structural grouping
    /// of [`Self::GridCell`] children in a [`Self::Grid`]. Non-
    /// interactive (no keyboard model, no AT actions); present so AT can
    /// expose the grid's row structure. Pinion's date picker paints its
    /// grid as flex rows, so the binding may emit `row` nodes only when
    /// a consumer needs explicit row grouping — the variant lands so the
    /// role surface is complete for future tabular widgets.
    Row,
    /// R863 §5.40 §5.27 §5.50 — WAI-ARIA 1.2 §3.3 `treegrid` role. A grid
    /// whose [`Self::Row`] children additionally carry the tree disclosure
    /// axes (`aria-level` / `aria-expanded` / `aria-posinset` / `aria-setsize`)
    /// — the textbook role for a hierarchical outliner with metadata columns
    /// (the self-hosted editor's scene outliner). Distinct from [`Self::Tree`]
    /// (a one-dimensional disclosure list with no columns) and [`Self::Grid`]
    /// (a flat two-dimensional grid with no disclosure): a `treegrid` is both,
    /// and AccessKit carries it as its own [`accesskit::Role::TreeGrid`].
    /// Pinion's first consumer is `view_virtual_treegrid` (R860); a
    /// `tree`+`treeitem` topology cannot expose the metadata columns as cells
    /// (a `treeitem` has no `gridcell` children in WAI-ARIA), so the columned
    /// tree lowers here instead.
    TreeGrid,
    /// R718 §5.40 — WAI-ARIA 1.2 §3.5 `progressbar` role. A
    /// descriptive (non-interactive) widget that reports the progress
    /// of a long-running task. Pairs with the §5.38
    /// [`ProgressBarExternal`](pinion_core::widgets::progress_bar) value
    /// holder + the R718 `hello-progress` consumer.
    ///
    /// The current fraction is carried by the
    /// [`AccessValue::Float`](crate::AccessValue::Float) value
    /// (`aria-valuenow` / `aria-valuemin` / `aria-valuemax`) — the same
    /// numeric lowering [`Self::Slider`] uses, so a determinate progress
    /// bar reports `value` in `[min, max]`. An *indeterminate* bar omits
    /// the value entirely (no `aria-valuenow`, the WAI-ARIA signal for
    /// "progress is unknown"); pinion's first slice is determinate-only,
    /// so the value is always present.
    ///
    /// Distinct from [`Self::Slider`] at the AT-action surface: a slider
    /// is operable (Focus / Increment / Decrement), while a progressbar
    /// is **passive** — it never receives focus and exposes no actions
    /// (it joins the zero-action arm with [`Self::Tooltip`] /
    /// [`Self::ColumnHeader`] / [`Self::Row`]). AT announces the value as
    /// a read-only status, not as an editable control.
    ProgressBar,
    /// R725 §5.40 — WAI-ARIA 1.2 §3.4 `status` role: a polite live
    /// region for advisory, transient messages (the Material 3
    /// snackbar / toast). AT announces the content when it appears
    /// without moving focus. Descriptive and **passive** — the
    /// container owns no keyboard model and exposes no AT actions
    /// (it joins the zero-action arm with [`Self::Tooltip`] /
    /// [`Self::ProgressBar`]); any in-snackbar action label is a
    /// separate [`Self::Button`] child with its own action surface.
    Status,
    /// R731 §5.40 — WAI-ARIA 1.2 §3.5 `navigation` landmark: a region of
    /// links for navigating the document or related documents (the
    /// breadcrumb trail, a nav rail). AT lists it among the page
    /// landmarks. Passive container — no keyboard model of its own (its
    /// link children carry the interaction).
    Navigation,
    /// R731 §5.40 — WAI-ARIA 1.2 §3.2 `link` role: an interactive
    /// reference that navigates when activated (a breadcrumb crumb). The
    /// current-location crumb additionally carries
    /// [`AccessNode::current`](crate::AccessNode::current) =
    /// `Some(AriaCurrent::Page)`.
    Link,
    /// R734 §5.40 — WAI-ARIA 1.2 §4.3 `spinbutton` role: a numeric input
    /// the user steps through a bounded range. Pairs with the §5.38
    /// `SpinButtonExternal` value holder (`hello-spinbutton`, R734). The
    /// value lowers through the same [`AccessValue::Float`] path
    /// (`aria-valuenow` / `aria-valuemin` / `aria-valuemax`) a `Slider`
    /// uses — `SpinButton` is the 3rd `Float` consumer after
    /// [`Self::Slider`] and [`Self::ProgressBar`]. Distinct from
    /// `Slider`: a slider is a *continuous* drag along a track, a
    /// spinbutton is a *discrete* stepped numeric field (ArrowUp/Down ±
    /// step, Home/End to min/max, PageUp/Down ± a larger step). It shares
    /// `Slider`'s **operable** AT action set (Focus + Increment +
    /// Decrement) — distinct from the *passive* `ProgressBar`, which
    /// reports a `Float` value but is read-only.
    ///
    /// [`AccessValue::Float`]: crate::node::AccessValue::Float
    SpinButton,
    /// R733 §5.40 — WAI-ARIA 1.2 §3.6 `group` role: a labelled set of
    /// user-interface objects that are *not* a page landmark (vs
    /// [`Self::Navigation`], which is). The canonical container for a
    /// **multi-select segmented button** / toggle-button group: each
    /// child is a [`Self::Button`] carrying `aria-pressed` (the
    /// `set_toggled` mapping on a `button` role — see the
    /// [`Self::Button`] doc for the `aria-pressed` vs `aria-checked`
    /// distinction). The group itself owns no keyboard model and
    /// exposes no AT action — it is a passive labelled wrapper so AT
    /// announces "View, group" before walking its toggle-button
    /// children. Pairs with the `hello-segmented-multi` consumer
    /// (R733). Distinct from [`Self::RadioGroup`] (single-select, where
    /// the children are mutually-exclusive `radio`s with
    /// `aria-checked`) and [`Self::Toolbar`] (which carries the
    /// roving-tabindex toolbar keyboard model); a plain `group` of
    /// independently-tabbable toggle buttons is the simplest WAI-ARIA
    /// multi-select toggle pattern.
    Group,
    /// R1551 §5.40 — WAI-ARIA 1.2 §4.3 `heading` role: a paragraph that
    /// declares itself a section title, carrying `aria-level` in
    /// [`AccessNode::level`](crate::AccessNode::level).
    ///
    /// Emitted by [`crate::attach_block_headings`] from a `TextNode` whose
    /// [`BlockFormat::heading_level`](pinion_core::style::BlockFormat::heading_level)
    /// is non-zero, so the announced structure is derived from the same
    /// declaration that indents and spaces the paragraph — one authority, not a
    /// heading marked twice.
    ///
    /// **the toolkit has the declaration and not the announcement.** `headingLevel()`
    /// exists (the toolkit 5.15+), but a text edit's accessibility surface is
    /// accessible text interface, whose whole vocabulary is character offsets,
    /// selections and text attributes — it has no method that reports block
    /// structure, so a toolkit document's heading levels reach its layout and
    /// stop there. Headings are the primary way a screen-reader user navigates
    /// a long document, which is what makes that gap worth crossing rather
    /// than matching.
    Heading,
    Generic,
}

impl AriaRole {
    /// Lower into `accesskit::Role` for `TreeUpdate` construction.
    ///
    /// `Generic` lowers to `Role::GenericContainer` per the WAI-ARIA
    /// authoring practices convention (it carries no implicit
    /// semantics; AT presents it as a passive container).
    #[must_use]
    pub const fn to_accesskit(self) -> Role {
        match self {
            Self::Heading => Role::Heading,
            Self::Button => Role::Button,
            Self::Switch => Role::Switch,
            Self::CheckBox => Role::CheckBox,
            Self::RadioButton => Role::RadioButton,
            Self::Slider => Role::Slider,
            Self::RadioGroup => Role::RadioGroup,
            Self::Listbox => Role::ListBox,
            Self::ListBoxOption => Role::ListBoxOption,
            // R714 §5.40 — AccessKit carries `combobox` one-to-one
            // (the select-only variant; `EditableComboBox` is the
            // typed-value future axis).
            Self::ComboBox => Role::ComboBox,
            // R717 §5.40 — AccessKit's typed-value combobox variant
            // (the editable sibling of `Role::ComboBox`).
            Self::EditableComboBox => Role::EditableComboBox,
            Self::TextInput => Role::TextInput,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::Tree => Role::Tree,
            Self::TreeItem => Role::TreeItem,
            Self::TabList => Role::TabList,
            Self::Tab => Role::Tab,
            Self::TabPanel => Role::TabPanel,
            Self::MenuBar => Role::MenuBar,
            Self::Menu => Role::Menu,
            Self::MenuItem => Role::MenuItem,
            Self::MenuItemCheckbox => Role::MenuItemCheckBox,
            Self::Toolbar => Role::Toolbar,
            Self::Dialog => Role::Dialog,
            Self::Tooltip => Role::Tooltip,
            // R704 §5.40 — AccessKit carries the interactive-grid roles
            // one-to-one (distinct from the static `Role::Table` family),
            // so the WAI-ARIA `grid` / `gridcell` / `columnheader` / `row`
            // split is preserved end to end.
            Self::Grid => Role::Grid,
            Self::GridCell => Role::GridCell,
            // R1560 §5.40 — AccessKit carries the static table roles as their
            // own arms, so the `table` / `cell` vs `grid` / `gridcell` split
            // survives to the platform adapter rather than being flattened
            // here.
            Self::Table => Role::Table,
            Self::Cell => Role::Cell,
            Self::ColumnHeader => Role::ColumnHeader,
            // R863 §5.40 §5.27 — AccessKit carries `rowheader` / `treegrid`
            // one-to-one, completing the columned-tree role set.
            Self::RowHeader => Role::RowHeader,
            Self::Row => Role::Row,
            Self::TreeGrid => Role::TreeGrid,
            // R718 §5.40 — AccessKit carries `progressbar` one-to-one
            // (`Role::ProgressIndicator`); the numeric value lowers
            // through the same `set_numeric_value` path as `Slider`.
            Self::ProgressBar => Role::ProgressIndicator,
            // R725 §5.40 — AccessKit carries `status` one-to-one as a
            // polite live region.
            Self::Status => Role::Status,
            Self::Navigation => Role::Navigation,
            Self::Link => Role::Link,
            // R734 §5.40 — AccessKit carries `spinbutton` one-to-one.
            Self::SpinButton => Role::SpinButton,
            // R733 §5.40 — AccessKit carries `group` one-to-one
            // (distinct from `GenericContainer`, which the
            // semantics-free `Generic` role maps to).
            Self::Group => Role::Group,
            Self::Generic => Role::GenericContainer,
        }
    }

    /// R1692 §5.40 — whether a region of this role that announces **nothing**
    /// is a defect.
    ///
    /// The voice census
    /// ([`NameFault::Absent`](pinion_core::voice::NameFault::Absent)) asks this
    /// before calling an empty name a fault, because for two roles emptiness is
    /// the answer rather than an omission:
    ///
    /// * a **cell** with no value is a blank cell. It is what the data says, a
    ///   screen reader announces it as blank, and there is nothing for an author
    ///   to repair. Measured when this landed: ten of one grid's cells are the
    ///   empty half of an optional column.
    /// * a **generic** container carries no semantics by definition — it is the
    ///   role for a box that is in the tree only to hold things.
    ///
    /// Deliberately **stricter than WAI-ARIA's own "Accessible Name Required"
    /// table**, which excuses far more (`group`, `list`, `toolbar`,
    /// `navigation`, `status` …). This is the floor-not-the-target rule applied
    /// to ourselves: a region we bothered to announce and cannot name is a
    /// region a reader lands on and learns nothing from, whatever the spec
    /// permits. The two exemptions above are the ones where a name would be an
    /// invention rather than a repair.
    ///
    /// Written as an **exhaustive match** rather than a `matches!` negation so a
    /// forty-third role cannot inherit an answer nobody chose: adding one stops
    /// this compiling, here, where the question is asked.
    #[must_use]
    pub const fn name_required(self) -> bool {
        match self {
            Self::Cell | Self::GridCell | Self::Generic => false,
            Self::Button
            | Self::Switch
            | Self::CheckBox
            | Self::RadioButton
            | Self::Slider
            | Self::RadioGroup
            | Self::Listbox
            | Self::ListBoxOption
            | Self::ComboBox
            | Self::EditableComboBox
            | Self::TextInput
            | Self::List
            | Self::ListItem
            | Self::Tree
            | Self::TreeItem
            | Self::TabList
            | Self::Tab
            | Self::TabPanel
            | Self::MenuBar
            | Self::Menu
            | Self::MenuItem
            | Self::MenuItemCheckbox
            | Self::Toolbar
            | Self::Dialog
            | Self::Tooltip
            | Self::Grid
            | Self::Table
            | Self::ColumnHeader
            | Self::RowHeader
            | Self::Row
            | Self::TreeGrid
            | Self::ProgressBar
            | Self::Status
            | Self::Navigation
            | Self::Link
            | Self::SpinButton
            | Self::Group
            | Self::Heading => true,
        }
    }

    /// ARIA literal name as it would appear in an HTML `role`
    /// attribute. Used by introspect schema so the RPC surface and
    /// the AT surface report identical role identifiers.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            // R1551 — the WAI-ARIA literal; the level rides `aria-level`, not
            // the role (HTML's `h1`..`h6` collapse to one ARIA role).
            Self::Heading => "heading",
            Self::Button => "button",
            Self::Switch => "switch",
            Self::CheckBox => "checkbox",
            Self::RadioButton => "radio",
            Self::Slider => "slider",
            Self::RadioGroup => "radiogroup",
            Self::Listbox => "listbox",
            Self::ListBoxOption => "option",
            // WAI-ARIA 1.2 has a single `combobox` role; the select-only
            // and editable variants (split only at the AccessKit layer)
            // both report the same literal token — editability is
            // conveyed by the input + `aria-autocomplete`, not the role.
            Self::ComboBox | Self::EditableComboBox => "combobox",
            // WAI-ARIA 1.2 spec literal — the single-line text input
            // role is `textbox` regardless of AccessKit's internal
            // single/multiline split.
            Self::TextInput => "textbox",
            Self::List => "list",
            Self::ListItem => "listitem",
            Self::Tree => "tree",
            Self::TreeItem => "treeitem",
            Self::TabList => "tablist",
            Self::Tab => "tab",
            Self::TabPanel => "tabpanel",
            Self::MenuBar => "menubar",
            Self::Menu => "menu",
            Self::MenuItem => "menuitem",
            Self::MenuItemCheckbox => "menuitemcheckbox",
            Self::Toolbar => "toolbar",
            Self::Dialog => "dialog",
            Self::Tooltip => "tooltip",
            Self::Grid => "grid",
            Self::GridCell => "gridcell",
            Self::Table => "table",
            Self::Cell => "cell",
            Self::ColumnHeader => "columnheader",
            Self::RowHeader => "rowheader",
            Self::Row => "row",
            Self::TreeGrid => "treegrid",
            Self::ProgressBar => "progressbar",
            Self::Status => "status",
            Self::Navigation => "navigation",
            Self::Link => "link",
            Self::SpinButton => "spinbutton",
            Self::Group => "group",
            Self::Generic => "generic",
        }
    }
}

/// R717 §5.40 — WAI-ARIA 1.2 §6.6.1 `aria-autocomplete`, the axis that
/// declares *how* an editable combobox predicts the user's intended
/// value as they type. Pinion-native mirror of `accesskit::AutoComplete`
/// (the wrapper keeps [`AccessNode`](crate::AccessNode) free of a direct
/// `accesskit` dependency, exactly as [`AriaRole`] does for `Role`).
///
/// `aria-autocomplete="none"` is modelled by the *absence* of the value
/// ([`AccessNode::auto_complete`](crate::AccessNode::auto_complete) =
/// `None`), matching AccessKit (which has no `None` member — an omitted
/// property means "none").
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AutoComplete {
    /// `aria-autocomplete="inline"` — the input is completed in place
    /// (selected text appended after the caret).
    Inline,
    /// `aria-autocomplete="list"` — a popup presents the filtered set
    /// of completions (the pinion editable-combobox typeahead model).
    List,
    /// `aria-autocomplete="both"` — list popup *and* inline completion.
    Both,
}

impl AutoComplete {
    /// Lower to `accesskit::AutoComplete`. The single bridge point so an
    /// `accesskit` minor bump rewrites only this arm.
    #[must_use]
    pub const fn to_accesskit(self) -> AkAutoComplete {
        match self {
            Self::Inline => AkAutoComplete::Inline,
            Self::List => AkAutoComplete::List,
            Self::Both => AkAutoComplete::Both,
        }
    }

    /// WAI-ARIA literal as it appears in an HTML `aria-autocomplete`
    /// attribute, so introspect / RPC and AT report identical tokens.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Inline => "inline",
            Self::List => "list",
            Self::Both => "both",
        }
    }
}

/// R730 §5.40 — WAI-ARIA 1.2 §6.6.2 `aria-sort` direction for a sortable
/// [`AriaRole::ColumnHeader`]. The wrapper keeps [`AccessNode`](crate::AccessNode) free of a
/// direct `accesskit` dependency, exactly as [`AutoComplete`] does.
///
/// `aria-sort="none"` (the column is sortable but not currently the sort
/// key) is modelled by the *absence* of the value
/// ([`AccessNode::sort`](crate::AccessNode::sort) = `None`). Only the two
/// directional states are carried — AccessKit's `Other` has no WAI-ARIA
/// column-sort meaning the data grid emits.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SortDirection {
    /// `aria-sort="ascending"` — rows ordered low → high by this column.
    Ascending,
    /// `aria-sort="descending"` — rows ordered high → low by this column.
    Descending,
}

impl SortDirection {
    /// R886 §5.40 — the `ascending: bool` → direction bridge. Every grid
    /// stores its sort key as `(col, ascending)` (the `grid_sort_str` wire
    /// vocabulary); this is the one home of the bool → `aria-sort` mapping
    /// the column-header builders previously each hand-rolled
    /// (hello-table / hello-grouped-grid-sort / hello-data-grid).
    #[must_use]
    pub const fn from_ascending(ascending: bool) -> Self {
        if ascending {
            Self::Ascending
        } else {
            Self::Descending
        }
    }

    /// Lower to `accesskit::SortDirection`. The single bridge point so an
    /// `accesskit` minor bump rewrites only this arm.
    #[must_use]
    pub const fn to_accesskit(self) -> AkSortDirection {
        match self {
            Self::Ascending => AkSortDirection::Ascending,
            Self::Descending => AkSortDirection::Descending,
        }
    }

    /// WAI-ARIA literal as it appears in an HTML `aria-sort` attribute, so
    /// introspect / RPC and AT report identical tokens.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Ascending => "ascending",
            Self::Descending => "descending",
        }
    }
}

/// R731 §5.40 — WAI-ARIA 1.2 §6.6.3 `aria-current`: which element in a set
/// of related elements is the *current* one. The wrapper keeps
/// [`AccessNode`](crate::AccessNode) free of a direct `accesskit` dependency, exactly as
/// [`SortDirection`] does.
///
/// `aria-current="false"` (not the current element) is modelled by the
/// *absence* of the value ([`AccessNode::current`](crate::AccessNode::current)
/// = `None`). Only the positive states are carried — the breadcrumb's
/// current crumb is [`Self::Page`].
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AriaCurrent {
    /// `aria-current="page"` — the current page within a navigation set
    /// (the breadcrumb's current-location crumb).
    Page,
    /// `aria-current="step"` — the current step in a process (a stepper).
    Step,
    /// `aria-current="location"` — the current location (e.g. an image map).
    Location,
    /// `aria-current="date"` — the current date in a calendar.
    Date,
    /// `aria-current="time"` — the current time in a timetable.
    Time,
    /// `aria-current="true"` — the current item, kind unspecified.
    True,
}

impl AriaCurrent {
    /// Lower to `accesskit::AriaCurrent`. The single bridge point so an
    /// `accesskit` minor bump rewrites only this arm.
    #[must_use]
    pub const fn to_accesskit(self) -> AkAriaCurrent {
        match self {
            Self::Page => AkAriaCurrent::Page,
            Self::Step => AkAriaCurrent::Step,
            Self::Location => AkAriaCurrent::Location,
            Self::Date => AkAriaCurrent::Date,
            Self::Time => AkAriaCurrent::Time,
            Self::True => AkAriaCurrent::True,
        }
    }

    /// WAI-ARIA literal as it appears in an HTML `aria-current` attribute,
    /// so introspect / RPC and AT report identical tokens.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Page => "page",
            Self::Step => "step",
            Self::Location => "location",
            Self::Date => "date",
            Self::Time => "time",
            Self::True => "true",
        }
    }
}

/// R985 §5.40 — WAI-ARIA 1.2 §6.6.5 `aria-haspopup`: declares that a control
/// owns a popup the activation opens. The wrapper keeps [`AccessNode`](crate::AccessNode) free of
/// a direct `accesskit` dependency, exactly as [`AutoComplete`] / [`SortDirection`]
/// do.
///
/// `aria-haspopup="false"` (no popup) is modelled by the *absence* of the value
/// ([`AccessNode::has_popup`](crate::AccessNode::has_popup) = `None`). The
/// canonical first consumer is the WAI-ARIA §3.16 menubar submenu: a
/// [`AriaRole::MenuItem`] that opens a child [`AriaRole::Menu`] carries
/// [`Self::Menu`], so AT announces "has submenu" and pairs it with the
/// `aria-expanded` state. The full closed set mirrors `accesskit::HasPopup`
/// (the fixed WAI-ARIA token list `menu | listbox | tree | grid | dialog`).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HasPopup {
    /// `aria-haspopup="menu"` — opens a menu (the submenu parent item).
    Menu,
    /// `aria-haspopup="listbox"` — opens a listbox (combobox popup).
    Listbox,
    /// `aria-haspopup="tree"` — opens a tree.
    Tree,
    /// `aria-haspopup="grid"` — opens a grid.
    Grid,
    /// `aria-haspopup="dialog"` — opens a dialog.
    Dialog,
}

impl HasPopup {
    /// Lower to `accesskit::HasPopup`. The single bridge point so an
    /// `accesskit` minor bump rewrites only this arm.
    #[must_use]
    pub const fn to_accesskit(self) -> AkHasPopup {
        match self {
            Self::Menu => AkHasPopup::Menu,
            Self::Listbox => AkHasPopup::Listbox,
            Self::Tree => AkHasPopup::Tree,
            Self::Grid => AkHasPopup::Grid,
            Self::Dialog => AkHasPopup::Dialog,
        }
    }

    /// WAI-ARIA literal as it appears in an HTML `aria-haspopup` attribute,
    /// so introspect / RPC and AT report identical tokens.
    #[must_use]
    pub const fn aria_name(self) -> &'static str {
        match self {
            Self::Menu => "menu",
            Self::Listbox => "listbox",
            Self::Tree => "tree",
            Self::Grid => "grid",
            Self::Dialog => "dialog",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn button_lowers_to_accesskit_button() {
        assert_eq!(AriaRole::Button.to_accesskit(), Role::Button);
    }

    #[test]
    fn switch_lowers_to_accesskit_switch() {
        assert_eq!(AriaRole::Switch.to_accesskit(), Role::Switch);
    }

    #[test]
    fn radio_group_lowers_to_accesskit_radio_group() {
        assert_eq!(AriaRole::RadioGroup.to_accesskit(), Role::RadioGroup);
    }

    #[test]
    fn slider_lowers_to_accesskit_slider() {
        assert_eq!(AriaRole::Slider.to_accesskit(), Role::Slider);
    }

    #[test]
    fn generic_lowers_to_generic_container() {
        assert_eq!(AriaRole::Generic.to_accesskit(), Role::GenericContainer);
    }

    #[test]
    fn r718_progress_bar_lowers_to_progress_indicator() {
        assert_eq!(
            AriaRole::ProgressBar.to_accesskit(),
            Role::ProgressIndicator
        );
        assert_eq!(AriaRole::ProgressBar.aria_name(), "progressbar");
    }

    #[test]
    fn r693_dialog_lowers_to_accesskit_dialog() {
        assert_eq!(AriaRole::Dialog.to_accesskit(), Role::Dialog);
        assert_eq!(AriaRole::Dialog.aria_name(), "dialog");
    }

    #[test]
    fn aria_names_match_wai_aria_literals() {
        assert_eq!(AriaRole::Button.aria_name(), "button");
        assert_eq!(AriaRole::Switch.aria_name(), "switch");
        assert_eq!(AriaRole::CheckBox.aria_name(), "checkbox");
        assert_eq!(AriaRole::RadioButton.aria_name(), "radio");
        assert_eq!(AriaRole::Slider.aria_name(), "slider");
        assert_eq!(AriaRole::RadioGroup.aria_name(), "radiogroup");
        assert_eq!(AriaRole::Listbox.aria_name(), "listbox");
        assert_eq!(AriaRole::ListBoxOption.aria_name(), "option");
        assert_eq!(AriaRole::TextInput.aria_name(), "textbox");
        assert_eq!(AriaRole::List.aria_name(), "list");
        assert_eq!(AriaRole::ListItem.aria_name(), "listitem");
        assert_eq!(AriaRole::Generic.aria_name(), "generic");
    }

    // R51.96.1 §5.40 — Listbox / Option role lowering.

    #[test]
    fn listbox_lowers_to_accesskit_listbox() {
        assert_eq!(AriaRole::Listbox.to_accesskit(), Role::ListBox);
    }

    #[test]
    fn listbox_option_lowers_to_accesskit_listbox_option() {
        assert_eq!(AriaRole::ListBoxOption.to_accesskit(), Role::ListBoxOption);
    }

    // R714 §5.40 — ComboBox role lowering + aria literal.

    #[test]
    fn combobox_lowers_to_accesskit_combobox() {
        assert_eq!(AriaRole::ComboBox.to_accesskit(), Role::ComboBox);
    }

    #[test]
    fn combobox_aria_name_is_combobox() {
        assert_eq!(AriaRole::ComboBox.aria_name(), "combobox");
    }

    // R717 §5.40 — EditableComboBox lowers to the AccessKit typed-value
    // variant, but the WAI-ARIA literal stays `combobox` (single role).
    #[test]
    fn r717_editable_combobox_lowers_to_editable_variant() {
        assert_eq!(
            AriaRole::EditableComboBox.to_accesskit(),
            Role::EditableComboBox
        );
    }

    #[test]
    fn r717_editable_combobox_aria_name_is_combobox() {
        assert_eq!(AriaRole::EditableComboBox.aria_name(), "combobox");
    }

    // R717 §5.40 — aria-autocomplete value enum bridge.
    #[test]
    fn r717_auto_complete_lowers_to_accesskit() {
        assert_eq!(AutoComplete::Inline.to_accesskit(), AkAutoComplete::Inline);
        assert_eq!(AutoComplete::List.to_accesskit(), AkAutoComplete::List);
        assert_eq!(AutoComplete::Both.to_accesskit(), AkAutoComplete::Both);
    }

    #[test]
    fn r717_auto_complete_aria_names_match_wai_aria() {
        assert_eq!(AutoComplete::Inline.aria_name(), "inline");
        assert_eq!(AutoComplete::List.aria_name(), "list");
        assert_eq!(AutoComplete::Both.aria_name(), "both");
    }

    // R56.1.b.1 §5.40 — TextInput role lowering.

    #[test]
    fn text_input_lowers_to_accesskit_text_input() {
        assert_eq!(AriaRole::TextInput.to_accesskit(), Role::TextInput);
    }

    // R656 §5.40 — List / ListItem role lowering.

    #[test]
    fn list_lowers_to_accesskit_list() {
        assert_eq!(AriaRole::List.to_accesskit(), Role::List);
    }

    #[test]
    fn list_item_lowers_to_accesskit_list_item() {
        assert_eq!(AriaRole::ListItem.to_accesskit(), Role::ListItem);
    }

    // R690 §5.40 — Tab / TabList / TabPanel role lowering + names.

    #[test]
    fn tab_list_lowers_to_accesskit_tab_list() {
        assert_eq!(AriaRole::TabList.to_accesskit(), Role::TabList);
    }

    #[test]
    fn tab_lowers_to_accesskit_tab() {
        assert_eq!(AriaRole::Tab.to_accesskit(), Role::Tab);
    }

    #[test]
    fn tab_panel_lowers_to_accesskit_tab_panel() {
        assert_eq!(AriaRole::TabPanel.to_accesskit(), Role::TabPanel);
    }

    #[test]
    fn tab_roles_aria_names_match_wai_aria_literals() {
        assert_eq!(AriaRole::TabList.aria_name(), "tablist");
        assert_eq!(AriaRole::Tab.aria_name(), "tab");
        assert_eq!(AriaRole::TabPanel.aria_name(), "tabpanel");
    }

    // R691 §5.40 — Menu / MenuBar / MenuItem role lowering + names.

    #[test]
    fn menu_bar_lowers_to_accesskit_menu_bar() {
        assert_eq!(AriaRole::MenuBar.to_accesskit(), Role::MenuBar);
    }

    #[test]
    fn menu_lowers_to_accesskit_menu() {
        assert_eq!(AriaRole::Menu.to_accesskit(), Role::Menu);
    }

    #[test]
    fn menu_item_lowers_to_accesskit_menu_item() {
        assert_eq!(AriaRole::MenuItem.to_accesskit(), Role::MenuItem);
    }

    #[test]
    fn r805_menu_item_checkbox_lowers_and_names() {
        assert_eq!(
            AriaRole::MenuItemCheckbox.to_accesskit(),
            Role::MenuItemCheckBox
        );
        assert_eq!(AriaRole::MenuItemCheckbox.aria_name(), "menuitemcheckbox");
    }

    #[test]
    fn menu_roles_aria_names_match_wai_aria_literals() {
        assert_eq!(AriaRole::MenuBar.aria_name(), "menubar");
        assert_eq!(AriaRole::Menu.aria_name(), "menu");
        assert_eq!(AriaRole::MenuItem.aria_name(), "menuitem");
    }

    // R692 §5.40 — Toolbar role lowering + name.

    #[test]
    fn toolbar_lowers_to_accesskit_toolbar() {
        assert_eq!(AriaRole::Toolbar.to_accesskit(), Role::Toolbar);
    }

    #[test]
    fn toolbar_aria_name_matches_wai_aria_literal() {
        assert_eq!(AriaRole::Toolbar.aria_name(), "toolbar");
    }

    // R695 §5.40 — Tooltip role lowering + name.

    #[test]
    fn r695_tooltip_lowers_to_accesskit_tooltip() {
        assert_eq!(AriaRole::Tooltip.to_accesskit(), Role::Tooltip);
        assert_eq!(AriaRole::Tooltip.aria_name(), "tooltip");
    }

    // R704 §5.40 — Grid / GridCell / ColumnHeader / Row lowering + names.

    #[test]
    fn r704_grid_lowers_to_accesskit_grid() {
        assert_eq!(AriaRole::Grid.to_accesskit(), Role::Grid);
        assert_eq!(AriaRole::Grid.aria_name(), "grid");
    }

    #[test]
    fn r704_grid_cell_lowers_to_accesskit_grid_cell() {
        assert_eq!(AriaRole::GridCell.to_accesskit(), Role::GridCell);
        assert_eq!(AriaRole::GridCell.aria_name(), "gridcell");
    }

    #[test]
    fn r704_column_header_lowers_to_accesskit_column_header() {
        assert_eq!(AriaRole::ColumnHeader.to_accesskit(), Role::ColumnHeader);
        assert_eq!(AriaRole::ColumnHeader.aria_name(), "columnheader");
    }

    #[test]
    fn r704_row_lowers_to_accesskit_row() {
        assert_eq!(AriaRole::Row.to_accesskit(), Role::Row);
        assert_eq!(AriaRole::Row.aria_name(), "row");
    }

    // R863 §5.40 §5.27 — RowHeader / TreeGrid lowering + names (the columned
    // tree-grid role set).

    #[test]
    fn r863_row_header_lowers_to_accesskit_row_header() {
        assert_eq!(AriaRole::RowHeader.to_accesskit(), Role::RowHeader);
        assert_eq!(AriaRole::RowHeader.aria_name(), "rowheader");
    }

    #[test]
    fn r863_tree_grid_lowers_to_accesskit_tree_grid() {
        assert_eq!(AriaRole::TreeGrid.to_accesskit(), Role::TreeGrid);
        assert_eq!(AriaRole::TreeGrid.aria_name(), "treegrid");
    }

    #[test]
    fn r734_spin_button_lowers_to_accesskit_spin_button() {
        // R734 §5.40 — discrete numeric stepper; shares Slider's Float
        // lowering + operable action set, distinct role token.
        assert_eq!(AriaRole::SpinButton.to_accesskit(), Role::SpinButton);
        assert_eq!(AriaRole::SpinButton.aria_name(), "spinbutton");
    }

    #[test]
    fn r733_group_lowers_to_accesskit_group() {
        // R733 §5.40 — labelled multi-select segmented-button container.
        // Distinct from `Generic` (→ `GenericContainer`, semantics-free).
        assert_eq!(AriaRole::Group.to_accesskit(), Role::Group);
        assert_eq!(AriaRole::Group.aria_name(), "group");
        assert_ne!(AriaRole::Group.to_accesskit(), Role::GenericContainer);
    }

    /// ★★★★ R1692 — which roles may announce nothing. Three, and each because
    /// emptiness there is the answer rather than an omission.
    ///
    /// A counterfactual is what asked for this test: `name_required` returning
    /// `false` for every role — which switches the whole `absent` rule off
    /// across 216 surfaces — compiled and passed every test in this tree. The
    /// exhaustive match handles the OTHER failure (a new role inheriting an
    /// answer nobody chose); this handles a wrong answer for an existing one.
    #[test]
    fn r1692_only_three_roles_may_announce_nothing() {
        for role in [AriaRole::Cell, AriaRole::GridCell, AriaRole::Generic] {
            assert!(
                !role.name_required(),
                "{} is the empty half of real data, not a defect",
                role.aria_name(),
            );
        }
        // The interactive roles and the structural ones a reader navigates to.
        // Deliberately wider than WAI-ARIA's own "Accessible Name Required"
        // table, which excuses `group`, `list`, `toolbar`, `navigation` and
        // `status`: a region we bothered to announce and cannot name is one a
        // reader lands on and learns nothing from.
        for role in [
            AriaRole::Button,
            AriaRole::Switch,
            AriaRole::CheckBox,
            AriaRole::Slider,
            AriaRole::SpinButton,
            AriaRole::TextInput,
            AriaRole::ComboBox,
            AriaRole::Link,
            AriaRole::Tab,
            AriaRole::TabPanel,
            AriaRole::ColumnHeader,
            AriaRole::RowHeader,
            AriaRole::Row,
            AriaRole::Tree,
            AriaRole::TreeItem,
            AriaRole::List,
            AriaRole::ListItem,
            AriaRole::Group,
            AriaRole::Status,
            AriaRole::Dialog,
            AriaRole::Heading,
            AriaRole::Table,
            AriaRole::Grid,
        ] {
            assert!(
                role.name_required(),
                "{} announcing nothing tells a reader only what it is, never \
                 which one",
                role.aria_name(),
            );
        }
    }
}
