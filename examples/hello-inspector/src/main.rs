//! `hello-inspector` — R909 §5.38 §5.22 selection-driven detail panel.
//!
//! ## What this demonstrates
//!
//! The central editor interaction loop — *select an object, inspect and
//! edit its properties* — that Unreal's "Details" panel, Unity's
//! Inspector, and Qt's `QtPropertyBrowser` are built around. The
//! `hello-property-grid` (R836) shows the typed-property machinery for
//! **one fixed object**; this binding adds the missing piece: a panel
//! whose property set is **driven by a selection** across several
//! heterogeneous objects (each with its own property schema), so
//! selecting a different object re-populates the inspector.
//!
//! - An object list (left) of scene entities — a Player, a Camera, a
//!   Light — each carrying a different typed [`CellValue`] property set.
//! - A detail panel (right) that reactively reflects the selected
//!   object: a header (its name) plus a row per property (name + a
//!   type-appropriate value visual — a colour swatch, an On/Off pill,
//!   text).
//! - Selection + editing over RPC (the §2 #2 AI-first primary path):
//!   `scene/invoke .../select {index}` selects, `scene/intervene
//!   .../value.<i>` edits the selected object's property (the
//!   [`CellValue::with_intervene`] typed-write — the 4th consumer of the
//!   `CellValue` substrate after property-grid, data-grid, and the
//!   node-editor port defaults).
//!
//! Per-object state is isolated: editing the Player's `Health` leaves
//! the Camera untouched, and re-selecting the Player shows the edit.
//!
//! ## Scope
//!
//! The novel contribution is the *selection → reactive inspector*
//! binding + per-object heterogeneous schemas. The value model and
//! typed write reuse [`CellValue`] wholesale (no re-implementation).
//! Selection is driven three ways — RPC (`invoke select` / `intervene
//! selected`), keyboard (Arrow / Home / End via `apply_key`), and a
//! pointer click on an object row (the R910 composite-send wire) — all
//! sharing one select funnel. Property *editing* is the AI-first RPC
//! `intervene value.<i>` path (the §2 primary); inline click-to-edit
//! cell delegates (the property-grid's popup/scrub richness) are the
//! remaining documented follow-up.
//!
//! ## Verification
//!
//! `tools/demos/r909_inspector.py` drives selection + editing over RPC
//! (select each object → its schema; intervene a property → re-query;
//! per-object isolation across re-selects). `tools/demos/r910_inspector_interaction.py`
//! drives the pointer click-to-select + keyboard navigation. All
//! scene-as-data, deterministic
//! ([[ai-first-rpc-introspection-obligation]],
//! [[introspection-from-paint-not-screen]]).

use std::rc::Rc;

use pinion_core::cell_value::CellValue;
use pinion_core::composite_tag::send_activation_index;
use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
};
use pinion_core::external::query_proxy_external_impl;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, FontWeight, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::{ColorRole, Frame, Owner, Scene, Signal, use_theme};
#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

// pinion-forge codegen output: `pub struct HelloInspectorRenderer`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// R51.30 — bridge the inherent renderer methods into the
// `pinion_shell::VelloRenderer` trait so `AppShell<V>` can drive it.
vello_renderer_impl!(HelloInspectorRenderer, HelloInspectorRendererError);

const WIN_W: u32 = 460;
const WIN_H: u32 = 420;

const INSPECTOR_TAG: &str = "inspector";
const THEME_TAG: &str = "app";

const TITLE_FONT_PX: u32 = 16;
const HEADER_FONT_PX: u32 = 15;
const ROW_FONT_PX: u32 = 13;

const LIST_W: u32 = 150;
const ROW_H: u32 = 30;
const SWATCH: u32 = 16;

// ─── Model ────────────────────────────────────────────────────────

/// One typed property: a display name plus its [`CellValue`].
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct Property {
    name: String,
    value: CellValue,
}

impl Property {
    fn new(name: &str, value: CellValue) -> Self {
        Self { name: name.to_owned(), value }
    }
}

/// A selectable scene object with its own typed property schema.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
struct ObjectData {
    name: String,
    properties: Vec<Property>,
}

impl ObjectData {
    fn new(name: &str, properties: Vec<Property>) -> Self {
        Self { name: name.to_owned(), properties }
    }
}

/// Three heterogeneous scene objects — each a different property schema,
/// which is what makes the inspector a *selection-driven* panel rather
/// than a fixed property grid.
fn default_objects() -> Vec<ObjectData> {
    vec![
        ObjectData::new(
            "Player",
            vec![
                Property::new("Visible", CellValue::Bool(true)),
                Property::new("Health", CellValue::Int(100)),
                Property::new("Speed", CellValue::Float(6.5)),
                Property::new(
                    "Team",
                    CellValue::Choice {
                        selected: 0,
                        options: vec!["Red".to_owned(), "Blue".to_owned(), "Neutral".to_owned()],
                    },
                ),
                Property::new("Tint", CellValue::Color(Color::rgb(0x4f, 0x9d, 0xff))),
            ],
        ),
        ObjectData::new(
            "Main Camera",
            vec![
                Property::new("Active", CellValue::Bool(true)),
                Property::new("Field of View", CellValue::Float(60.0)),
                Property::new(
                    "Projection",
                    CellValue::Choice {
                        selected: 0,
                        options: vec!["Perspective".to_owned(), "Orthographic".to_owned()],
                    },
                ),
            ],
        ),
        ObjectData::new(
            "Sun Light",
            vec![
                Property::new("Enabled", CellValue::Bool(true)),
                Property::new("Intensity", CellValue::Float(1.2)),
                Property::new("Color", CellValue::Color(Color::rgb(0xff, 0xe0, 0x9a))),
                Property::new("Cast Shadows", CellValue::Bool(false)),
            ],
        ),
    ]
}

/// The shared object model — the same `Rc<Signal>` the
/// [`InspectorExternal`] mutates and the view reads ([[reactive-holder-for-shared-external-view-state]]).
fn use_objects() -> Rc<Signal<Vec<ObjectData>>> {
    let owner = Owner::current().expect("use_objects requires an active Owner scope");
    owner.cache("inspector.objects", || Signal::new(default_objects()))
}

/// The shared selection index (clamped to the object range by the
/// External's `select`).
fn use_selected() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_selected requires an active Owner scope");
    owner.cache("inspector.selected", || Signal::new(0usize))
}

// ─── External (the §5.15 AI surface) ──────────────────────────────

/// The inspector coordinator: owns the object model + selection, and
/// exposes selection-relative property query / intervene plus a `select`
/// action. The selection-relative addressing (`value.<i>` resolves
/// against the *selected* object) is what distinguishes this from the
/// fixed `PropertyGridExternal`.
struct InspectorExternal {
    objects: Rc<Signal<Vec<ObjectData>>>,
    selected: Rc<Signal<usize>>,
}

impl InspectorExternal {
    fn new(objects: Rc<Signal<Vec<ObjectData>>>, selected: Rc<Signal<usize>>) -> Self {
        Self { objects, selected }
    }

    fn object_count(&self) -> usize {
        self.objects.get().len()
    }

    /// The selected index, clamped to a valid object (the model is never
    /// empty, but a stale selection past a shrunk model clamps).
    fn sel(&self) -> usize {
        self.selected.get().min(self.object_count().saturating_sub(1))
    }

    /// Select object `idx`. `false` (no change) if out of range.
    fn select(&self, idx: usize) -> bool {
        if idx < self.object_count() {
            self.selected.set(idx);
            true
        } else {
            false
        }
    }

    /// Property count of the selected object.
    fn row_count(&self) -> usize {
        self.objects.get().get(self.sel()).map_or(0, |o| o.properties.len())
    }

    /// Typed write into the selected object's property `idx`, reusing
    /// [`CellValue::with_intervene`] (the same path the property grid /
    /// data grid use) so a choice sets its option by index and a colour
    /// parses a hex string.
    fn set_property(&self, idx: usize, value: IntrospectValue) -> Result<(), InterveneError> {
        let sel = self.sel();
        let current = self
            .objects
            .get()
            .get(sel)
            .and_then(|o| o.properties.get(idx))
            .map(|p| p.value.clone())
            .ok_or(InterveneError::UnknownPath)?;
        let new_value = current.with_intervene(value)?;
        self.objects.set_with(move |prev| {
            let mut next = prev.clone();
            next[sel].properties[idx].value = new_value.clone();
            next
        });
        Ok(())
    }
}

impl core::fmt::Debug for InspectorExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("InspectorExternal")
            .field("object_count", &self.object_count())
            .field("selected", &self.sel())
            .finish()
    }
}

// R913.1 — the Gui+Rpc / Framework / UiThreadSync `impl External`
// skeleton is the `query_proxy_external_impl!` SSOT (a config-holder
// External whose state is a shared reactive holder); hand-rolling it was
// a [[use-substrate-not-hand-rolled-equivalent]] miss. The macro needs
// the `ExternalIntrospect` impl below.
query_proxy_external_impl!(InspectorExternal);

impl ExternalIntrospect for InspectorExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("object_count", "int"),
            ("selected", "int"),
            ("selected_name", "string"),
            ("row_count", "int"),
            ("object_name.<j>", "string"),
            ("name.<i>", "string"),
            ("kind.<i>", "string"),
            ("value.<i>", "json"),
            ("select", "int"),
            ("send", "string"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let objects = self.objects.get();
        let sel = self.sel();
        match path {
            "object_count" => Some(IntrospectValue::Int(i64::try_from(objects.len()).ok()?)),
            "selected" => Some(IntrospectValue::Int(i64::try_from(sel).ok()?)),
            "selected_name" => objects.get(sel).map(|o| IntrospectValue::Text(o.name.clone())),
            "row_count" => Some(IntrospectValue::Int(i64::try_from(self.row_count()).ok()?)),
            _ => {
                if let Some(j) = path.strip_prefix("object_name.") {
                    let j: usize = j.parse().ok()?;
                    return objects.get(j).map(|o| IntrospectValue::Text(o.name.clone()));
                }
                let props = &objects.get(sel)?.properties;
                if let Some(i) = path.strip_prefix("name.") {
                    let i: usize = i.parse().ok()?;
                    return props.get(i).map(|p| IntrospectValue::Text(p.name.clone()));
                }
                if let Some(i) = path.strip_prefix("kind.") {
                    let i: usize = i.parse().ok()?;
                    return props.get(i).map(|p| IntrospectValue::Text(p.value.kind().name().to_owned()));
                }
                if let Some(i) = path.strip_prefix("value.") {
                    let i: usize = i.parse().ok()?;
                    return props.get(i).map(|p| p.value.to_introspect());
                }
                None
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "object_count" | "selected_name" | "row_count" => Err(InterveneError::ReadOnly),
            "selected" => match value {
                IntrospectValue::Int(n) => {
                    let idx = usize::try_from(n).map_err(|_| InterveneError::OutOfRange)?;
                    if self.select(idx) {
                        Ok(())
                    } else {
                        Err(InterveneError::OutOfRange)
                    }
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            _ => {
                let Some(idx_str) = path.strip_prefix("value.") else {
                    return Err(InterveneError::UnknownPath);
                };
                let idx: usize = idx_str.parse().map_err(|_| InterveneError::UnknownPath)?;
                self.set_property(idx, value)
            }
        }
    }

    fn invoke(&mut self, path: &str, args: IntrospectValue) -> Result<IntrospectValue, InvokeError> {
        match path {
            "select" => match args {
                IntrospectValue::Int(n) => {
                    let idx = usize::try_from(n).map_err(|_| InvokeError::TypeMismatch)?;
                    if self.select(idx) {
                        Ok(IntrospectValue::Int(n))
                    } else {
                        Err(InvokeError::Rejected)
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // R910 — the composite send wire: a click on an `inspector#<i>`
            // object row (or the keyboard nav in `apply_key`) routes here
            // as `"<i>:<EventName>"`. Select on the activation edge
            // (PointerUp / KeyboardActivate), ignoring the enter/down/leave
            // half of the pointer cycle so hover does not select.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    if let Some(idx) = send_activation_index(payload) {
                        self.select(idx);
                    }
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn make_inspector_external() -> InspectorExternal {
    InspectorExternal::new(use_objects(), use_selected())
}

/// Resolve the keyboard target object index for a nav key, read off the
/// External's introspect (`object_count` + `selected`). Arrow Down/Right
/// = next, Up/Left = previous (both clamp), Home/End = first/last.
/// `None` for a non-nav key or an empty model.
fn resolve_target_index(intro: Option<&dyn ExternalIntrospect>, key: &str) -> Option<usize> {
    let intro = intro?;
    let count = match intro.query("object_count")? {
        IntrospectValue::Int(n) => usize::try_from(n).ok()?,
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let last = count - 1;
    let cur = match intro.query("selected")? {
        IntrospectValue::Int(n) => usize::try_from(n).unwrap_or(0),
        _ => 0,
    };
    match key {
        "ArrowDown" | "ArrowRight" => Some((cur + 1).min(last)),
        "ArrowUp" | "ArrowLeft" => Some(cur.saturating_sub(1)),
        "Home" => Some(0),
        "End" => Some(last),
        _ => None,
    }
}

// ─── View ─────────────────────────────────────────────────────────

/// A type-appropriate value visual for the right column of a property
/// row: a swatch for a colour, an On/Off pill for a bool, the display
/// text otherwise.
fn value_visual(value: &CellValue, fg: Color, accent: Color, muted: Color) -> Scene {
    match value {
        CellValue::Color(c) => {
            let swatch = Scene::Container(
                ContainerNode::new(Vec::new())
                    .with_style(BoxStyle::filled(*c).with_corner_radius(3))
                    .with_layout(LayoutStyle::new().with_size(Size::px(SWATCH, SWATCH))),
            );
            let hex = Scene::Text(TextNode::styled(
                c.to_hex(),
                Rect::default(),
                TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(fg),
            ));
            Scene::Container(
                ContainerNode::new(vec![swatch, hex]).with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_align_items(AlignItems::Center)
                        .with_gap(6),
                ),
            )
        }
        CellValue::Bool(b) => {
            let (label, fill) = if *b { ("On", accent) } else { ("Off", muted) };
            Scene::Container(
                ContainerNode::new(vec![Scene::Text(TextNode::styled(
                    label,
                    Rect::default(),
                    TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(Color::rgb(0xff, 0xff, 0xff)),
                ))])
                .with_style(BoxStyle::filled(fill).with_corner_radius(8))
                .with_layout(
                    LayoutStyle::new()
                        .flex(FlexDirection::Row)
                        .with_justify(JustifyContent::Center)
                        .with_align_items(AlignItems::Center)
                        .with_size(Size::px(40, 20)),
                ),
            )
        }
        other => Scene::Text(TextNode::styled(
            other.display(),
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(fg),
        )),
    }
}

/// One inspector property row: `name` (left, muted) + value visual
/// (right). Tagged `prop_<i>` so the demo can locate it.
fn property_row(index: usize, prop: &Property, fg: Color, muted: Color, accent: Color) -> Scene {
    let name = Scene::Text(TextNode::styled(
        prop.name.clone(),
        Rect::default(),
        TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(muted),
    ));
    Scene::Container(
        ContainerNode::new(vec![name, value_visual(&prop.value, fg, accent, muted)])
            .with_tag(format!("prop_{index}"))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::SpaceBetween)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(260, ROW_H)),
            ),
    )
}

/// One object-list entry. The selected one carries an accent background.
fn object_row(index: usize, name: &str, selected: bool, fg: Color, accent: Color) -> Scene {
    let fill = if selected { accent } else { Color::TRANSPARENT };
    let text_fg = if selected { Color::rgb(0xff, 0xff, 0xff) } else { fg };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            name.to_owned(),
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(text_fg),
        ))])
        // Composite tag `inspector#<i>`: the input router's `#` split
        // (R51.42) routes a click here to the primary External's `send`
        // wire as `"<i>:<EventName>"` — the hello-tabs / radio-group
        // click-to-select pattern.
        .with_tag(format!("{INSPECTOR_TAG}#{index}"))
        .with_style(BoxStyle::filled(fill).with_corner_radius(5))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(LIST_W - 16, ROW_H)),
        ),
    )
}

/// view-fn (§6.3): pure sync. `selected` comes from the External's
/// introspect (`read_state`); the object data is read reactively.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(selected: usize, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let surface_alt = theme.resolve(ColorRole::SurfaceContainerHighest);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);

    let objects = use_objects().get();
    let sel = selected.min(objects.len().saturating_sub(1));

    let title = Scene::Text(TextNode::styled(
        "Scene Inspector",
        Rect::default(),
        TextStyle::new().with_size_px(TITLE_FONT_PX).with_weight(FontWeight::BOLD).with_fg(on_surface),
    ));

    // Left: object list.
    let list_rows: Vec<Scene> = objects
        .iter()
        .enumerate()
        .map(|(i, o)| object_row(i, &o.name, i == sel, on_surface, accent))
        .collect();
    let list = Scene::Container(
        ContainerNode::new(list_rows)
            .with_style(BoxStyle::filled(surface_alt).with_corner_radius(6))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(4)
                    .with_size(Size::px(LIST_W, WIN_H - 70)),
            ),
    );

    // Right: detail panel for the selected object.
    let selected_obj = objects.get(sel);
    let header = Scene::Text(TextNode::styled(
        selected_obj.map_or_else(|| "—".to_owned(), |o| o.name.clone()),
        Rect::default(),
        TextStyle::new().with_size_px(HEADER_FONT_PX).with_weight(FontWeight::BOLD).with_fg(on_surface),
    ));
    let mut detail_children = vec![header];
    if let Some(obj) = selected_obj {
        for (i, prop) in obj.properties.iter().enumerate() {
            detail_children.push(property_row(i, prop, on_surface, muted, accent));
        }
    }
    let detail = Scene::Container(
        ContainerNode::new(detail_children)
            .with_tag("detail_panel")
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(6)
                    .with_size(Size::px(WIN_W - LIST_W - 40, WIN_H - 70)),
            ),
    );

    let panes = Scene::Container(
        ContainerNode::new(vec![list, detail]).with_layout(
            LayoutStyle::new().flex(FlexDirection::Row).with_gap(16).with_align_items(AlignItems::Start),
        ),
    );

    Scene::Container(
        ContainerNode::new(vec![title, panes])
            .with_tag(INSPECTOR_TAG)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(12)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ─── Binding ──────────────────────────────────────────────────────

/// `WidgetView` binding. `#[widget]` derives the [`WidgetCore`] /
/// `WidgetA11y` / `WidgetView` trio; the selection lives in
/// [`InspectorExternal`] and is read back through introspect.
///
/// [`WidgetCore`]: pinion_core::WidgetCore
#[widget(
    tag = "inspector",
    state = usize,
    event = (),
    title = "pinion hello-inspector (R909 §5.38 detail panel)",
    renderer = HelloInspectorRenderer,
    initial_size = (WIN_W, WIN_H),
    external = make_inspector_external,
    role = Group,
    apply_key,
)]
struct InspectorView;

impl InspectorView {
    /// Read the selection index off the primary External's introspect.
    fn read_state(scene: &Scene) -> usize {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Int(n)) = intro.query("selected") {
                    return usize::try_from(n).unwrap_or(0);
                }
            }
        }
        0
    }

    fn view(state: usize, frame: Frame) -> Scene {
        view(state, &frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Keyboard navigation over the object list: Arrow keys move the
    /// selection (clamped), Home/End jump to the ends. Routes through the
    /// External's `send` wire (`"<i>:KeyboardActivate"`) so keyboard and
    /// pointer share the one select path.
    fn apply_key(
        scene: &mut Scene,
        _focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        let Some(target) = resolve_target_index(node.handle.introspect(), key) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let _ = intro.invoke("send", IntrospectValue::Text(format!("{target}:KeyboardActivate")));
        true
    }
}

fn main() {
    pinion_shell::run::<InspectorView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> InspectorExternal {
        let owner = Owner::new();
        owner.run(make_inspector_external)
    }

    #[test]
    fn r909_default_selection_is_first_object() {
        let e = ext();
        assert_eq!(e.object_count(), 3);
        assert_eq!(e.sel(), 0);
        assert_eq!(e.query("selected_name"), Some(IntrospectValue::Text("Player".to_owned())));
    }

    #[test]
    fn r909_select_re_targets_the_property_set() {
        let mut e = ext();
        // Player has 5 props, Main Camera has 3.
        assert_eq!(e.row_count(), 5);
        e.invoke("select", IntrospectValue::Int(1)).unwrap();
        assert_eq!(e.sel(), 1);
        assert_eq!(e.row_count(), 3);
        assert_eq!(e.query("selected_name"), Some(IntrospectValue::Text("Main Camera".to_owned())));
        assert_eq!(e.query("name.0"), Some(IntrospectValue::Text("Active".to_owned())));
    }

    #[test]
    fn r909_select_out_of_range_is_rejected() {
        let mut e = ext();
        assert!(e.invoke("select", IntrospectValue::Int(9)).is_err());
        assert_eq!(e.sel(), 0, "selection unchanged after a rejected select");
    }

    #[test]
    fn r909_intervene_edits_the_selected_object_only() {
        let mut e = ext();
        // Edit Player.Health (index 1) to 42.
        e.intervene("value.1", IntrospectValue::Int(42)).unwrap();
        assert_eq!(e.query("value.1"), Some(IntrospectValue::Int(42)));
        // Camera (object 1) is untouched: its value.1 is Field of View.
        e.invoke("select", IntrospectValue::Int(1)).unwrap();
        assert_eq!(e.query("value.1"), Some(IntrospectValue::Float(60.0)));
        // Re-select Player: the edit persisted (per-object state).
        e.invoke("select", IntrospectValue::Int(0)).unwrap();
        assert_eq!(e.query("value.1"), Some(IntrospectValue::Int(42)));
    }

    #[test]
    fn r909_intervene_selected_axis_moves_the_selection() {
        let mut e = ext();
        e.intervene("selected", IntrospectValue::Int(2)).unwrap();
        assert_eq!(e.query("selected_name"), Some(IntrospectValue::Text("Sun Light".to_owned())));
        assert!(e.intervene("selected", IntrospectValue::Int(99)).is_err());
    }

    #[test]
    fn r910_send_wire_selects_on_activation_edge() {
        let mut e = ext();
        // The full pointer cycle a composite click produces: only the
        // PointerUp activation edge selects.
        e.invoke("send", IntrospectValue::Text("2:PointerEnter".to_owned())).unwrap();
        assert_eq!(e.sel(), 0, "hover (PointerEnter) must not select");
        e.invoke("send", IntrospectValue::Text("2:PointerDown".to_owned())).unwrap();
        assert_eq!(e.sel(), 0, "press (PointerDown) must not select");
        e.invoke("send", IntrospectValue::Text("2:PointerUp".to_owned())).unwrap();
        assert_eq!(e.sel(), 2, "release (PointerUp) selects");
        assert_eq!(e.query("selected_name"), Some(IntrospectValue::Text("Sun Light".to_owned())));
    }

    #[test]
    fn r910_keyboard_nav_clamps_at_both_ends() {
        let mut e = ext();
        let activate = |e: &mut InspectorExternal, idx: usize| {
            e.invoke("send", IntrospectValue::Text(format!("{idx}:KeyboardActivate"))).unwrap();
        };
        // resolve_target_index drives the index; here we exercise the
        // clamp arithmetic the keyboard path applies.
        assert_eq!(resolve_target_index(Some(&e), "ArrowDown"), Some(1));
        activate(&mut e, 1);
        assert_eq!(resolve_target_index(Some(&e), "End"), Some(2));
        activate(&mut e, 2);
        assert_eq!(resolve_target_index(Some(&e), "ArrowDown"), Some(2), "clamps at last");
        assert_eq!(resolve_target_index(Some(&e), "Home"), Some(0));
        activate(&mut e, 0);
        assert_eq!(resolve_target_index(Some(&e), "ArrowUp"), Some(0), "clamps at first");
        assert_eq!(resolve_target_index(Some(&e), "Tab"), None, "non-nav key ignored");
    }

    #[test]
    fn r909_object_names_addressable_regardless_of_selection() {
        let e = ext();
        assert_eq!(e.query("object_name.0"), Some(IntrospectValue::Text("Player".to_owned())));
        assert_eq!(e.query("object_name.2"), Some(IntrospectValue::Text("Sun Light".to_owned())));
        assert_eq!(e.query("object_name.9"), None);
    }

    #[test]
    fn r909_read_only_axes_reject_intervene() {
        let mut e = ext();
        assert_eq!(e.intervene("object_count", IntrospectValue::Int(1)), Err(InterveneError::ReadOnly));
        assert_eq!(e.intervene("row_count", IntrospectValue::Int(1)), Err(InterveneError::ReadOnly));
    }

    fn has_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content == needle,
            Scene::Container(c) => c.children.iter().any(|ch| has_text(ch, needle)),
            _ => false,
        }
    }

    #[test]
    fn view_reflects_selected_object() {
        let owner = Owner::new();
        let scene = owner.run(|| view(2, &Frame::new()));
        // The Sun Light panel must carry its name + first property.
        assert!(has_text(&scene, "Sun Light"), "header names the selected object");
        assert!(has_text(&scene, "Intensity"), "selected object's property shown");
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<InspectorView>(0, &Frame::new());
    }

    #[test]
    fn inspector_root_reports_group_role() {
        let nodes = <InspectorView as WidgetA11y>::access_node(&0, None);
        assert_eq!(nodes[0].role, AriaRole::Group);
        assert_eq!(nodes[0].tag, INSPECTOR_TAG);
    }
}
