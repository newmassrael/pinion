//! R683.B §5.16 §5.41 — backend-agnostic Dock-panel primitive +
//! drag-to-tear-off [`External`].
//!
//! ## Role
//!
//! A **`DockPanel`** is the atomic unit of a multi-pane DCC / IDE /
//! CAD layout (the Phase B → D north star surface). Each panel
//! carries a header strip the user can grab + drag past a threshold
//! to **tear it off** into a new floating window — the canonical
//! pro-tool authoring affordance every Photoshop / Figma / Unreal
//! Editor / `VSCode` panel system ships.
//!
//! v1 ships the **panel primitive** + tear-off detection only. The
//! topology composition (recursive split tree across
//! `DockSlot::{Left, Right, Top, Bottom, Center}` with nested
//! splitters) is application-level for v1, composed via
//! [`splitter::view_splitter`](crate::splitter::view_splitter) +
//! [`view_dock_panel`]. The substrate-as-topology lift is deferred
//! per [[abstraction-needs-second-consumer]] — R683.B atomic 4's
//! `hello-dock-panels` is the 1st consumer; a 2nd consumer with a
//! different topology shape (e.g. an `editor` binding with main
//! viewport + outliner + properties + console + asset browser) will
//! surface the topology-level abstraction's actual contract.
//!
//! ## Tear-off wire
//!
//! [`DockPanelExternal`] captures the pointer on `PointerDown`
//! against the panel's header tag. Each `pointer_move` under
//! capture lock checks the cursor distance from the press-time
//! frame against [`DockPanelStyle::tear_off_threshold_frac`]; when
//! the threshold is crossed the external emits a `tear_off` intent
//! with the panel id as `IntrospectValue::Text` payload. The
//! intent fires exactly once per drag (subsequent moves past the
//! threshold do not re-fire).
//!
//! The binding's [`WidgetCore::update`](pinion_core::WidgetCore::update)
//! reducer matches against the dotted wire form
//! `{panel_tag}.tear_off` (per
//! [[intent-tag-dotted-wire-form]]) and on a successful match
//! pushes a new [`WindowSpec`](pinion_shell::WindowSpec) onto its
//! reactive `Signal<Vec<WindowSpec>>` (R683.A reconcile Effect
//! picks it up and a 2nd window appears with the torn-off panel's
//! content).
//!
//! ## Why intent-based tear-off (not direct `WindowSpec` push)
//!
//! The dock substrate cannot push `WindowSpec`s directly — Phase B
//! crate boundary discipline (`pinion-widget-paint` sits below
//! `pinion-shell`; reaching across would create a downward
//! reverse-dep). The intent channel is the canonical mechanism for
//! widget → binding signalling (mirror of every other widget's
//! event emission); the binding holds the
//! `Signal<Vec<WindowSpec>>` + the dock panel descriptors and
//! translates intents into topology mutations.
//!
//! ## Dep graph
//!
//! Sits beside [`splitter`](crate::splitter) (one tier above
//! [`text_field`](crate::text_field) / [`tree_view`](crate::tree_view)
//! since it composes via Splitter). Pure
//! [`Scene`](pinion_core::Scene) composition, no `pinion-text`
//! dependency, no Vello / winit coupling.

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;

use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, ThreadOwnership,
};
use pinion_core::intent::Intent;
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme};

/// R683.B §5.16 — symbolic event name the
/// [`DockPanelExternal`] emits when the user's drag exceeds the
/// configured threshold. Constant (not raw literal) so binding-side
/// reducer match arms can spell the dotted intent tag via
/// [`intent_tag!`](pinion_core::intent_tag) without duplicating the
/// literal: `intent_tag!(PANEL_TAG, dock::TEAR_OFF_EVENT)`.
pub const TEAR_OFF_EVENT: &str = "tear_off";

/// R683.B §5.16 — sidecar carrying [`view_dock_panel`]'s
/// binding-local visual + behavioural constants. `#[non_exhaustive]`
/// so future axes (resize handles, close button, collapse arrow)
/// land via builders without breaking the constructor surface.
///
/// Use [`Self::m3_default`] for the M3-canonical 28-px header strip
/// plus a 0.5 tear-off-threshold-fraction (the user must drag the header across
/// half its own width before the tear-off intent fires — matches the
/// `VSCode` / `IntelliJ` pane tear-off feel).
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct DockPanelStyle {
    /// Header strip extent (logical pixels) along the cross axis of
    /// the panel (height for the default `FlexDirection::Column`
    /// layout). Material 3 list / app-bar dense-row convention is
    /// 28 px; pro-tool authoring surfaces (DCC / IDE panels) use
    /// 24-32 px for compactness.
    pub header_height_px: u32,
    /// Fraction of the header extent the cursor must travel from
    /// the press point before [`TEAR_OFF_EVENT`] fires. Default
    /// `0.5` — half the header width matches the implicit
    /// `VSCode` / `JetBrains` feel (the user has to commit to the
    /// drag).
    ///
    /// Cursor delta is computed via the L∞ norm (`max(|Δx_rel|,
    /// |Δy_rel|)`) so diagonal drag past either axis fires. Pure
    /// horizontal or pure vertical drag through `Δx_rel =
    /// tear_off_threshold_frac` is the canonical UX trigger.
    pub tear_off_threshold_frac: f32,
    /// Paint-side tag the panel's outer
    /// [`Scene::Container`] carries. The header strip is tagged
    /// `{tag}#header` (composite-tag convention R51.42); the
    /// content area is tagged `{tag}#content`. The
    /// [`DockPanelExternal`] is registered against the header tag
    /// so deepest-tagged hit-test routes `PointerDown` on the
    /// header to it.
    pub tag: Cow<'static, str>,
    /// Font size for the header title text. M3 label-medium token
    /// = 12 sp by default; reads tightly against the 28-px header
    /// strip.
    pub header_font_size_px: u32,
}

impl DockPanelStyle {
    /// (R683.B §5.16) M3-canonical default: 28-px header, 0.5
    /// tear-off fraction, 12-px header font.
    #[must_use]
    pub fn m3_default(tag: impl Into<Cow<'static, str>>) -> Self {
        Self {
            header_height_px: 28,
            tear_off_threshold_frac: 0.5,
            tag: tag.into(),
            header_font_size_px: 12,
        }
    }

    /// Override the tear-off threshold fraction. Floor `0.0` makes
    /// the tear-off fire on the very first `pointer_move`; ceiling
    /// `1.0` requires the cursor to drag a full header-extent past
    /// the press point before firing. Out-of-range inputs degrade
    /// the UX but do not abort (the L∞ delta saturates at `1.0`
    /// inside the header rect; under capture lock `x_rel` / `y_rel`
    /// can exceed `[0.0, 1.0]`).
    #[must_use]
    pub fn with_tear_off_threshold_frac(mut self, frac: f32) -> Self {
        self.tear_off_threshold_frac = frac;
        self
    }

    /// Override the header strip height in logical pixels. Touch
    /// surfaces want ≥ 44 px (Material touch-target floor).
    #[must_use]
    pub const fn with_header_height_px(mut self, height: u32) -> Self {
        self.header_height_px = height;
        self
    }
}

/// (R683.B §5.16) Composite-tag suffix for the dock panel's header
/// strip. The header is the drag-able surface; the
/// [`DockPanelExternal`] attaches to the composite tag
/// `{panel_tag}#header` so `PointerDown` on the header routes to it
/// (deepest-tagged hit-test).
pub const HEADER_TAG_SUFFIX: &str = "header";

/// (R683.B §5.16) Composite-tag suffix for the dock panel's content
/// area. Always present so AI clients can introspect the panel's
/// inner content tree via `scene/snapshot {path: "{panel_tag}#content"}`.
pub const CONTENT_TAG_SUFFIX: &str = "content";

/// (R683.B §5.16) Backend-agnostic dock-panel composition.
///
/// Builds a vertical [`Scene::Container`] (`FlexDirection::Column`)
/// with two children:
///
/// 1. Header strip — fixed `header_height_px` tall, tagged
///    `{panel_tag}#header`, M3 `SurfaceContainerHigh` fill,
///    contains a single [`TextNode`] with the panel's `title`.
/// 2. Content area — flex-grow 1, tagged
///    `{panel_tag}#content`, transparent fill, wraps the
///    application-supplied `content` Scene.
///
/// The outer Container carries [`DockPanelStyle::tag`] (the panel's
/// canonical id) so AI introspection + future dock topology code
/// can locate the panel root.
///
/// The header strip is the drag handle for tear-off: the
/// [`DockPanelExternal`] the binding registers against the
/// `{tag}#header` composite-tag receives `PointerDown` + tracks
/// drag distance + emits [`TEAR_OFF_EVENT`] past the threshold.
///
/// # Panics
///
/// Never panics on its own — `title` is borrowed verbatim into a
/// `TextNode`; `content` is moved into the content container
/// without inspection.
#[must_use]
pub fn view_dock_panel(
    title: &str,
    content: Scene,
    theme: &Theme,
    style: &DockPanelStyle,
) -> Scene {
    let header_tag = composite_tag(&style.tag, HEADER_TAG_SUFFIX);
    let content_tag = composite_tag(&style.tag, CONTENT_TAG_SUFFIX);
    let header_title = Scene::Text(TextNode::styled(
        title.to_string(),
        Rect::default(),
        TextStyle::new()
            .with_size_px(style.header_font_size_px)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));
    let header = Scene::Container(
        ContainerNode::new(vec![header_title])
            .with_tag(header_tag)
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerHigh),
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_size(Size::px(0, style.header_height_px))
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Start)
                    .with_padding(Rect::new(8, 0, 8, 0)),
            ),
    );
    let content_wrapper = Scene::Container(
        ContainerNode::new(vec![content])
            .with_tag(content_tag)
            .with_layout(LayoutStyle::new().with_flex_grow(1.0)),
    );
    Scene::Container(
        ContainerNode::new(vec![header, content_wrapper])
            .with_tag(style.tag.clone())
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainer),
            ))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch),
            ),
    )
}

fn composite_tag(panel_tag: &str, suffix: &'static str) -> String {
    format!("{panel_tag}#{suffix}")
}

/// (R683.B §5.16) Cursor snapshot captured on the first
/// `pointer_move` under capture lock. The drag distance is computed
/// as `(x_rel - cursor_x_at_press, y_rel - cursor_y_at_press)` each
/// subsequent frame; the L∞ norm (`max(|Δx|, |Δy|)`) crosses the
/// `tear_off_threshold_frac` to fire the intent.
#[derive(Debug, Clone, Copy)]
struct DockDragStart {
    cursor_x: f32,
    cursor_y: f32,
}

/// (R683.B §5.16) Drag-to-tear-off External for the
/// [`view_dock_panel`] header strip. Registered by the binding via
/// [`WidgetCore::create_extra_externals`](pinion_core::WidgetCore::create_extra_externals)
/// tagged with the composite tag `{panel_tag}#header` (matching the
/// view fn's header child).
///
/// ## Wire
///
/// `wants_pointer_capture = true` so the cursor lock survives the
/// press → drag → release span (the user can drag well past the
/// header strip before tear-off fires). Each `pointer_move` under
/// capture lock checks the L∞ delta from the press-time frame
/// against [`DockPanelStyle::tear_off_threshold_frac`]; on first
/// crossing the external emits a
/// [`TEAR_OFF_EVENT`] intent with the panel id as the
/// `IntrospectValue::Text` payload + sets the `fired_for_drag`
/// guard so subsequent moves do not re-fire.
///
/// `PointerUp` / `PointerCancel` (delivered via the
/// [`ExternalIntrospect::invoke`] `"send"` channel) clear the drag
/// snapshot + the `fired_for_drag` guard so the next press starts a
/// fresh cycle.
///
/// ## Pattern of operations
///
/// 1. Construct: `DockPanelExternal::new(panel_id, threshold_frac)`.
/// 2. Application's `create_extra_externals` registers the
///    external against the composite header tag
///    (`{panel_tag}#header`).
/// 3. User presses + drags past the threshold — the external
///    emits the `tear_off` intent.
/// 4. Binding's `WidgetCore::update` reducer catches the dotted
///    intent (`{panel_tag}#header.tear_off`) + pushes a fresh
///    `WindowSpec` onto its `Signal<Vec<WindowSpec>>`.
/// 5. R683.A `reconcile_windows` Effect picks up the signal change +
///    spawns the new floating window with the torn-off panel's content.
pub struct DockPanelExternal {
    /// Stable panel identifier carried into the tear-off intent
    /// payload. The binding's reducer + the
    /// `Signal<Vec<WindowSpec>>` push use this to determine which
    /// panel was torn off + what content the new window should
    /// host.
    panel_id: Cow<'static, str>,
    /// Tear-off threshold as a fraction of the header rect extent
    /// (matches [`DockPanelStyle::tear_off_threshold_frac`]). The
    /// external receives a copy of the style's value at
    /// construction so the threshold can be queried + introspected.
    tear_off_threshold_frac: f32,
    /// Drag-start snapshot. `None` between presses; `Some` once
    /// the first `pointer_move` under capture lock calibrates.
    drag_start: Cell<Option<DockDragStart>>,
    /// Whether the `tear_off` intent has already been emitted for
    /// the current drag. Guards against multi-fire (every
    /// `pointer_move` past the threshold would otherwise re-fire,
    /// pushing N+1 `WindowSpec`s per single user drag).
    fired_for_drag: Cell<bool>,
    /// Pending intents waiting for the framework's
    /// [`External::drain_intents`] poll. v1 fires exactly one
    /// `tear_off` per drag, so the queue depth is `≤ 1` in steady
    /// state, but the `VecDeque` shape leaves room for future
    /// multi-event drags (e.g. an `tear_off_armed` precursor +
    /// `tear_off` final).
    pending_intents: RefCell<VecDeque<Intent>>,
}

impl core::fmt::Debug for DockPanelExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DockPanelExternal")
            .field("panel_id", &self.panel_id)
            .field("tear_off_threshold_frac", &self.tear_off_threshold_frac)
            .field("drag_start", &self.drag_start.get())
            .field("fired_for_drag", &self.fired_for_drag.get())
            .finish_non_exhaustive()
    }
}

impl DockPanelExternal {
    /// Construct a dock-panel tear-off External for the given
    /// panel id + threshold. The threshold must match
    /// [`DockPanelStyle::tear_off_threshold_frac`] the view fn
    /// uses — they are paired (visual + drag detection) for the
    /// canonical UX.
    #[must_use]
    pub fn new(
        panel_id: impl Into<Cow<'static, str>>,
        tear_off_threshold_frac: f32,
    ) -> Self {
        Self {
            panel_id: panel_id.into(),
            tear_off_threshold_frac,
            drag_start: Cell::new(None),
            fired_for_drag: Cell::new(false),
            pending_intents: RefCell::new(VecDeque::new()),
        }
    }

    /// Read the panel id this external carries — the payload the
    /// `tear_off` intent ships.
    #[must_use]
    pub fn panel_id(&self) -> &str {
        &self.panel_id
    }

    /// Read the tear-off threshold fraction.
    #[must_use]
    pub fn tear_off_threshold_frac(&self) -> f32 {
        self.tear_off_threshold_frac
    }

    /// Diagnostic: drag-in-progress flag. `true` between the
    /// press-time `pointer_move` calibration and the `PointerUp`
    /// / `PointerCancel` clear.
    #[must_use]
    pub fn is_dragging(&self) -> bool {
        self.drag_start.get().is_some()
    }

    /// Diagnostic: whether the `tear_off` intent has fired for the
    /// current drag. `false` until the threshold is crossed; back
    /// to `false` after the release clears the cycle.
    #[must_use]
    pub fn tear_off_fired(&self) -> bool {
        self.fired_for_drag.get()
    }

    /// Pure projection: compute the L∞ cursor delta against the
    /// press-time snapshot. Returns `None` before the drag
    /// calibrates. Exposed `pub(crate)` for unit tests; not part of
    /// the public surface.
    pub(crate) fn cursor_delta_l_inf(&self, x_rel: f32, y_rel: f32) -> Option<f32> {
        let snapshot = self.drag_start.get()?;
        let dx = (x_rel - snapshot.cursor_x).abs();
        let dy = (y_rel - snapshot.cursor_y).abs();
        Some(dx.max(dy))
    }

    /// Enqueue the `tear_off` intent. Internal helper —
    /// `pointer_move` calls this exactly once per drag when the
    /// threshold is crossed.
    fn enqueue_tear_off(&self) {
        self.pending_intents.borrow_mut().push_back(Intent {
            tag: Cow::Borrowed(TEAR_OFF_EVENT),
            payload: IntrospectValue::Text(self.panel_id.to_string()),
        });
    }
}

impl External for DockPanelExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    /// Capture lock so the cursor stays pinned to the header strip
    /// for the duration of the press, even when the cursor strays
    /// outside the header rect (the natural tear-off path —
    /// dragging the panel out of its dock slot toward a new window
    /// position).
    fn wants_pointer_capture(&self) -> bool {
        true
    }

    /// R51.34 §5.15 + §5.35 — calibrate drag-start on the first
    /// frame, accumulate delta on subsequent frames, fire
    /// [`TEAR_OFF_EVENT`] intent once when the L∞ delta crosses
    /// [`DockPanelStyle::tear_off_threshold_frac`].
    fn pointer_move(&mut self, x_rel: f32, y_rel: f32) {
        if self.drag_start.get().is_none() {
            self.drag_start.set(Some(DockDragStart {
                cursor_x: x_rel,
                cursor_y: y_rel,
            }));
            return;
        }
        if self.fired_for_drag.get() {
            // Tear-off already fired — no more work for this drag.
            // The binding's reducer will have already pushed a new
            // WindowSpec; subsequent cursor jitter must not re-fire.
            return;
        }
        let Some(delta) = self.cursor_delta_l_inf(x_rel, y_rel) else {
            return;
        };
        if delta >= self.tear_off_threshold_frac {
            self.enqueue_tear_off();
            self.fired_for_drag.set(true);
        }
    }

    fn is_dirty(&self) -> bool {
        !self.pending_intents.borrow().is_empty()
    }

    fn drain_intents(&mut self, sink: &mut dyn FnMut(Intent)) {
        let mut queue = self.pending_intents.borrow_mut();
        while let Some(intent) = queue.pop_front() {
            sink(intent);
        }
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for DockPanelExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("panel_id", "string"),
            ("tear_off_threshold_frac", "float"),
            ("dragging", "bool"),
            ("tear_off_fired", "bool"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "panel_id" => Some(IntrospectValue::Text(self.panel_id.to_string())),
            "tear_off_threshold_frac" => {
                Some(IntrospectValue::Float(f64::from(self.tear_off_threshold_frac)))
            }
            "dragging" => Some(IntrospectValue::Bool(self.is_dragging())),
            "tear_off_fired" => Some(IntrospectValue::Bool(self.tear_off_fired())),
            _ => None,
        }
    }

    fn intervene(
        &mut self,
        path: &str,
        _value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        match path {
            // Every slot is framework-owned or construction-time
            // fixed. AI clients drive the tear-off through the
            // `invoke("send", ...)` channel + the binding's
            // reducer + the windows_signal push — not by
            // intervening on dragging / tear_off_fired directly.
            "panel_id" | "tear_off_threshold_frac" | "dragging" | "tear_off_fired" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    /// R51.41 §5.15 §5.35 — framework synthetic event channel.
    ///
    /// `PointerUp` / `PointerCancel` clear the drag state +
    /// tear-off-fired guard. `PointerDown` / `PointerEnter` /
    /// `PointerLeave` arrive but are no-ops at this level (drag
    /// calibration happens on the first `pointer_move`).
    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if path != "send" {
            return Err(InvokeError::UnknownPath);
        }
        let event_name = args.as_str().ok_or(InvokeError::TypeMismatch)?;
        match event_name {
            "PointerUp" | "PointerCancel" => {
                self.drag_start.set(None);
                self.fired_for_drag.set(false);
                Ok(IntrospectValue::Null)
            }
            "PointerDown" | "PointerEnter" | "PointerLeave" => Ok(IntrospectValue::Null),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

#[cfg(test)]
mod tests {
    //! R683.B §5.16 — Dock-panel paint + tear-off wire tests.
    //!
    //! Pins the load-bearing invariants the
    //! [`hello-dock-panels`](crate) + future `DockSurface` consumers
    //! rely on:
    //!
    //! 1. **Paint shape**: outer Container carries `tag` + 2
    //!    children (header strip + content wrapper). Header tagged
    //!    `{tag}#header`, content tagged `{tag}#content`.
    //! 2. **Header height**: header child's layout matches
    //!    `header_height_px` style.
    //! 3. **Header text**: header contains a `TextNode` with the
    //!    supplied title.
    //! 4. **Tear-off threshold default**: 0.5 (M3 default).
    //! 5. **Drag calibration**: first `pointer_move` snapshots; no
    //!    intent fires.
    //! 6. **Threshold crossing**: drag past threshold fires exactly
    //!    one `tear_off` intent with the panel id payload.
    //! 7. **Single-fire guard**: subsequent moves past threshold do
    //!    not re-fire.
    //! 8. **`PointerUp` clears state**: `drag_start` + fired guard
    //!    both reset on the canonical release.
    //! 9. **Threshold not reached**: short drag → release → no
    //!    intent fired.
    //! 10. **L∞ delta semantics**: diagonal drag fires when EITHER
    //!     axis crosses the threshold.
    //! 11. **Introspect schema + query**: `panel_id` / threshold /
    //!     `dragging` / `tear_off_fired` all queryable.
    //! 12. **Composite tag format**: `{tag}#header` /
    //!     `{tag}#content`.

    use super::{
        composite_tag, view_dock_panel, DockPanelExternal, DockPanelStyle,
        CONTENT_TAG_SUFFIX, HEADER_TAG_SUFFIX, TEAR_OFF_EVENT,
    };
    use pinion_core::external::{External, ExternalIntrospect, IntrospectValue};
    use pinion_core::intent::Intent;
    use pinion_core::reactive::Owner;
    use pinion_core::scene::{ContainerNode, Scene};
    use pinion_core::theme::Theme;

    const PANEL_TAG: &str = "test_panel";

    fn run_in_owner<R>(f: impl FnOnce() -> R) -> R {
        Owner::new().run(f)
    }

    fn empty_content() -> Scene {
        Scene::Container(ContainerNode::new(vec![]).with_tag("test_panel_content_payload"))
    }

    fn theme_light() -> Theme {
        Theme::light()
    }

    #[test]
    fn r683_view_dock_panel_outer_container_carries_tag_and_two_children() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("My Panel", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            assert_eq!(outer.tag.as_deref(), Some(PANEL_TAG));
            assert_eq!(outer.children.len(), 2);
        });
    }

    #[test]
    fn r683_view_dock_panel_header_tagged_with_composite_suffix() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(header) = &outer.children[0] else { panic!() };
            assert_eq!(
                header.tag.as_deref(),
                Some(composite_tag(PANEL_TAG, HEADER_TAG_SUFFIX).as_str()),
            );
        });
    }

    #[test]
    fn r683_view_dock_panel_content_tagged_with_composite_suffix() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(content) = &outer.children[1] else { panic!() };
            assert_eq!(
                content.tag.as_deref(),
                Some(composite_tag(PANEL_TAG, CONTENT_TAG_SUFFIX).as_str()),
            );
        });
    }

    #[test]
    fn r683_view_dock_panel_header_height_matches_style() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG).with_header_height_px(32);
            let scene = view_dock_panel("Title", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(header) = &outer.children[0] else { panic!() };
            // size.height is a SizeValue::Px(32) — match the
            // numeric extent via the layout.size field.
            let height_px = match header.layout.size.height {
                pinion_core::style::SizeValue::Px(px) => Some(px),
                _ => None,
            };
            assert_eq!(height_px, Some(32));
        });
    }

    #[test]
    fn r683_view_dock_panel_header_contains_title_text() {
        run_in_owner(|| {
            let style = DockPanelStyle::m3_default(PANEL_TAG);
            let scene = view_dock_panel("Inspector", empty_content(), &theme_light(), &style);
            let Scene::Container(outer) = &scene else { panic!() };
            let Scene::Container(header) = &outer.children[0] else { panic!() };
            // Header has exactly one child: the title TextNode.
            assert_eq!(header.children.len(), 1);
            let Scene::Text(text) = &header.children[0] else { panic!() };
            assert_eq!(text.content, "Inspector");
        });
    }

    #[test]
    fn r683_dock_panel_style_m3_default_carries_canonical_defaults() {
        let style = DockPanelStyle::m3_default(PANEL_TAG);
        assert_eq!(style.header_height_px, 28);
        assert!((style.tear_off_threshold_frac - 0.5).abs() < f32::EPSILON);
        assert_eq!(style.header_font_size_px, 12);
        assert_eq!(style.tag.as_ref(), PANEL_TAG);
    }

    #[test]
    fn r683_dock_panel_style_with_tear_off_threshold_override() {
        let style = DockPanelStyle::m3_default(PANEL_TAG).with_tear_off_threshold_frac(0.25);
        assert!((style.tear_off_threshold_frac - 0.25).abs() < f32::EPSILON);
    }

    #[test]
    fn r683_dock_panel_external_first_pointer_move_calibrates_no_intent() {
        let mut ext = DockPanelExternal::new("inspector_panel", 0.5);
        ext.pointer_move(0.3, 0.5);
        assert!(ext.is_dragging());
        assert!(!ext.tear_off_fired());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(
            received.is_empty(),
            "press-time frame must not enqueue any intent",
        );
    }

    #[test]
    fn r683_dock_panel_external_drag_past_threshold_fires_tear_off() {
        let mut ext = DockPanelExternal::new("inspector_panel", 0.5);
        // Press at (0.3, 0.5); move to (0.85, 0.5) — Δx = 0.55, past 0.5.
        ext.pointer_move(0.3, 0.5);
        ext.pointer_move(0.85, 0.5);
        assert!(ext.tear_off_fired());
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1, "exactly one tear_off per drag");
        assert_eq!(received[0].tag.as_ref(), TEAR_OFF_EVENT);
        assert_eq!(received[0].payload.as_str(), Some("inspector_panel"));
    }

    #[test]
    fn r683_dock_panel_external_subsequent_moves_past_threshold_do_not_refire() {
        let mut ext = DockPanelExternal::new("p1", 0.3);
        ext.pointer_move(0.0, 0.5);
        ext.pointer_move(0.5, 0.5); // crosses threshold, fires once
        // Continue dragging further — must NOT re-fire.
        ext.pointer_move(0.7, 0.5);
        ext.pointer_move(0.9, 0.5);
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(
            received.len(),
            1,
            "multi-fire guard must keep total tear_offs = 1 per drag",
        );
    }

    #[test]
    fn r683_dock_panel_external_pointer_up_clears_drag_state() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        ext.pointer_move(0.3, 0.5);
        ext.pointer_move(0.85, 0.5); // fires
        assert!(ext.tear_off_fired());
        // Drain the intent off the queue (mirror of framework's
        // per-frame drain) before checking state — the drain
        // empties the queue but does not clear the
        // tear_off_fired guard (the guard clears on PointerUp).
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert_eq!(received.len(), 1);
        // PointerUp clears both.
        ext.invoke("send", IntrospectValue::Text("PointerUp".to_string()))
            .expect("invoke send PointerUp returns Ok");
        assert!(!ext.is_dragging());
        assert!(!ext.tear_off_fired());
    }

    #[test]
    fn r683_dock_panel_external_pointer_cancel_also_clears() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        ext.pointer_move(0.3, 0.5);
        assert!(ext.is_dragging());
        ext.invoke("send", IntrospectValue::Text("PointerCancel".to_string()))
            .expect("invoke send PointerCancel returns Ok");
        assert!(!ext.is_dragging());
    }

    #[test]
    fn r683_dock_panel_external_short_drag_no_intent() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        ext.pointer_move(0.5, 0.5);
        // Move only 0.2 — under threshold 0.5.
        ext.pointer_move(0.7, 0.5);
        let mut received: Vec<Intent> = Vec::new();
        ext.drain_intents(&mut |i| received.push(i));
        assert!(
            received.is_empty(),
            "drag below threshold must not enqueue tear_off",
        );
        assert!(!ext.tear_off_fired());
    }

    #[test]
    fn r683_dock_panel_external_l_inf_diagonal_drag_fires_on_y_axis_too() {
        // L∞ norm: max(|Δx|, |Δy|). Pure y drag past threshold
        // must fire (the canonical "drag panel down out of slot"
        // gesture).
        let mut ext = DockPanelExternal::new("p1", 0.4);
        ext.pointer_move(0.5, 0.0);
        // Cursor moves down by 0.5 (above threshold 0.4) but x
        // unchanged.
        ext.pointer_move(0.5, 0.5);
        assert!(ext.tear_off_fired(), "y-axis drag past threshold must fire");
    }

    #[test]
    fn r683_dock_panel_external_cursor_delta_l_inf_pre_calibration_is_none() {
        let ext = DockPanelExternal::new("p1", 0.5);
        // Before any pointer_move call, no snapshot exists.
        assert!(ext.cursor_delta_l_inf(0.5, 0.5).is_none());
    }

    #[test]
    fn r683_dock_panel_external_introspect_schema_includes_canonical_paths() {
        let ext = DockPanelExternal::new("p1", 0.5);
        let schema = ext.schema();
        let fields: Vec<&str> = schema.fields.iter().map(|(n, _)| *n).collect();
        for needed in [
            "panel_id",
            "tear_off_threshold_frac",
            "dragging",
            "tear_off_fired",
            "send",
        ] {
            assert!(fields.contains(&needed), "schema must include {needed}");
        }
    }

    #[test]
    fn r683_dock_panel_external_query_panel_id() {
        let ext = DockPanelExternal::new("my_panel", 0.5);
        let val = ext.query("panel_id").expect("queryable");
        assert_eq!(val.as_str(), Some("my_panel"));
    }

    #[test]
    fn r683_dock_panel_external_query_tear_off_fired_starts_false() {
        let ext = DockPanelExternal::new("p1", 0.5);
        let val = ext.query("tear_off_fired").expect("queryable");
        assert_eq!(val, IntrospectValue::Bool(false));
    }

    #[test]
    fn r683_dock_panel_external_invoke_unknown_event_returns_err() {
        let mut ext = DockPanelExternal::new("p1", 0.5);
        let res = ext.invoke("send", IntrospectValue::Text("UnknownEvent".to_string()));
        assert!(res.is_err());
    }

    #[test]
    fn r683_composite_tag_format_matches_input_router_convention() {
        // R51.42 §5.35 — the composite-tag convention is
        // `{primary}#{suffix}`. The dock panel's header + content
        // tags both follow this format so the InputRouter's
        // deepest-tagged hit-test + dispatch_send wire route
        // PointerDown to the matching External.
        assert_eq!(composite_tag("panel_a", HEADER_TAG_SUFFIX), "panel_a#header");
        assert_eq!(composite_tag("panel_a", CONTENT_TAG_SUFFIX), "panel_a#content");
    }

    #[test]
    fn r683_dock_panel_external_panel_id_accessor_returns_construction_value() {
        let ext = DockPanelExternal::new("inspector", 0.5);
        assert_eq!(ext.panel_id(), "inspector");
    }

    #[test]
    fn r683_dock_panel_external_threshold_accessor() {
        let ext = DockPanelExternal::new("p1", 0.42);
        assert!((ext.tear_off_threshold_frac() - 0.42).abs() < f32::EPSILON);
    }
}
