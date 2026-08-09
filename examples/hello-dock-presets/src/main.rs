// R1412 §5.49 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-dock-presets` — R1412 §5.49 — a named **dock-layout preset manager**
//! and the **forcing consumer** of the R1412 validated
//! [`DockTopology`] `Deserialize`.
//!
//! ## What this demonstrates
//!
//! A professional editor (the engine / the DCC / an IDE) lets a user save a
//! window arrangement under a name and switch between saved "workspaces" /
//! "layouts" / "perspectives". pinion's dock topology is a serde-serializable
//! [`DockTopology`] held in a reactive `Signal`, so a *preset* is just a stored topology and
//! *applying* one is a `Signal::set`. This demo is that manager:
//!
//!   * **apply** a named preset — deserialize its stored blob and swap the live
//!     topology; the dock surface relayouts reactively.
//!   * **save** the current layout under a name — serialize the live topology
//!     into a new named blob.
//!   * **delete** a named preset.
//!
//! Presets are stored as **serialized `DockTopology` JSON blobs**, exactly as an
//! on-disk / cross-session persistence layer would keep them, and applying one
//! runs `serde_json::from_str::<DockTopology>` — the R1412 seam.
//!
//! ## The forcing consumer (why the `Deserialize` is hand-written)
//!
//! `DockTopology` keeps its `root` field private: every constructor routes
//! through `try_new`, whose validation gate guarantees unique panel / split /
//! tabs ids and canonical wells — invariants every walker relies on. A *derived*
//! `Deserialize` would populate `root` directly and skip that gate, so a
//! corrupt / hostile blob could reconstruct an INVALID topology. R1412 makes
//! `Deserialize` a hand-written impl that routes the parsed tree through
//! `try_new`, so a persisted layout is as trustworthy as a constructed one.
//! This demo proves it end-to-end: the `"corrupt"` preset is a blob with a
//! duplicate panel id; **applying it is REJECTED** (the status line says so and
//! the live topology is unchanged), not silently applied as a broken layout.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! The preset manager is driven entirely over RPC: the `presets` external
//! answers `names` / `active` / `status` / `count` / `active_blob` and takes
//! `apply` / `save` / `delete` invokes. So an AI can enumerate the presets,
//! switch layouts, and read back which one is live with no pixel. See
//! `tools/demos/r1412_dock_presets.py`.

use std::rc::Rc;

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, LayoutStyle, Size, SizeValue, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::{Frame, Owner, Scene, Signal, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};
use pinion_widget_paint::dock::{DockNode, DockSplitState, DockTopology, view_dock_surface};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloDockPresetsRenderer, HelloDockPresetsRendererError);

const WIN_W: u32 = 920;
const WIN_H: u32 = 600;
const BAR_H: u32 = 56;
const THEME_TAG: &str = "app";

/// The primary (window-root) external's registration tag. It is a passive
/// surface — every command flows through the [`PresetCommandExternal`].
const ROOT_TAG: &str = "root";

/// The preset-manager command external's tag — addressed over RPC as
/// `/presets/external/<field>`.
const PRESETS_TAG: &str = "presets";

const STATUS_TAG: &str = "status_line";
const ACTIVE_TAG: &str = "active_line";

/// The three demo panels. Each preset arranges exactly these ids. Only the
/// tests enumerate the set (the builders name the leaves directly).
#[cfg(test)]
const PANELS: [&str; 3] = ["outline", "canvas", "props"];

const TITLE_FONT_PX: u32 = 15;
const STATUS_FONT_PX: u32 = 13;

// ── built-in preset topologies ───────────────────────────────────────────
// Distinct split ids per preset (w*/t*/e*) so a preset's ratio Signal never
// bleeds into another's `Owner::cache` slot.

/// `outline | canvas | props` — all three panels side by side.
fn build_wide() -> DockTopology {
    DockTopology::new(DockNode::split_horizontal(
        "w0",
        0.34,
        DockNode::leaf("outline"),
        DockNode::split_horizontal("w1", 0.5, DockNode::leaf("canvas"), DockNode::leaf("props")),
    ))
}

/// `outline / canvas / props` — all three panels stacked vertically.
fn build_tall() -> DockTopology {
    DockTopology::new(DockNode::split_vertical(
        "t0",
        0.34,
        DockNode::leaf("outline"),
        DockNode::split_vertical("t1", 0.5, DockNode::leaf("canvas"), DockNode::leaf("props")),
    ))
}

/// `outline | (canvas / props)` — a narrow outline rail beside a stacked
/// canvas + props. The default / boot layout.
fn build_editor() -> DockTopology {
    DockTopology::new(DockNode::split_horizontal(
        "e0",
        0.25,
        DockNode::leaf("outline"),
        DockNode::split_vertical("e1", 0.7, DockNode::leaf("canvas"), DockNode::leaf("props")),
    ))
}

/// A deliberately INVALID topology blob: two leaves share the panel id
/// `outline`. Built by serializing a raw [`DockNode`] tree (which has no
/// validation gate — only [`DockTopology::try_new`] does) into the
/// `{ "root": ... }` wire shape, so the blob's field names are exactly the
/// serde form, never hand-guessed. Applying it must be REJECTED by the R1412
/// validated `Deserialize`.
fn corrupt_blob() -> String {
    let raw = DockNode::split_horizontal(
        "c0",
        0.5,
        DockNode::leaf("outline"),
        DockNode::leaf("outline"),
    );
    let wrapped = serde_json::json!({ "root": serde_json::to_value(&raw).expect("node -> json") });
    serde_json::to_string(&wrapped).expect("wrapper -> json string")
}

// ── demo-local reactive state (shared via Owner::cache) ───────────────────

fn use_topology() -> Rc<Signal<Option<DockTopology>>> {
    Owner::current()
        .expect("view / external run inside the root owner")
        .cache("topology", || Signal::new(Some(build_editor())))
}

/// The named preset store: `(name, serialized DockTopology JSON)`. Seeded with
/// the three built-ins plus a `"corrupt"` witness blob.
fn use_presets() -> Rc<Signal<Vec<(String, String)>>> {
    Owner::current()
        .expect("view / external run inside the root owner")
        .cache("presets", || {
            Signal::new(vec![
                ("editor".to_owned(), serialize_topology(&build_editor())),
                ("wide".to_owned(), serialize_topology(&build_wide())),
                ("tall".to_owned(), serialize_topology(&build_tall())),
                ("corrupt".to_owned(), corrupt_blob()),
            ])
        })
}

fn use_active() -> Rc<Signal<String>> {
    Owner::current()
        .expect("view / external run inside the root owner")
        .cache("active", || Signal::new("editor".to_owned()))
}

fn use_status() -> Rc<Signal<String>> {
    Owner::current()
        .expect("view / external run inside the root owner")
        .cache("status", || Signal::new("ready".to_owned()))
}

fn use_split_ratio(key: String, initial: f32) -> Rc<Signal<f32>> {
    Owner::current()
        .expect("view runs inside the root owner")
        .cache(key, || Signal::new(initial))
}

fn serialize_topology(topo: &DockTopology) -> String {
    serde_json::to_string(topo).expect("a valid topology serializes")
}

// ── preset operations (shared state handles; no Owner needed at call site) ─

/// The reactive handles the [`PresetCommandExternal`] mutates. Cloned from the
/// same `Owner::cache` slots the view reads, so a command repaints the surface.
struct PresetSignals {
    topology: Rc<Signal<Option<DockTopology>>>,
    presets: Rc<Signal<Vec<(String, String)>>>,
    active: Rc<Signal<String>>,
    status: Rc<Signal<String>>,
}

impl PresetSignals {
    fn stored_blob(&self, name: &str) -> Option<String> {
        self.presets
            .get()
            .into_iter()
            .find(|(n, _)| n == name)
            .map(|(_, blob)| blob)
    }

    /// Apply a named preset: deserialize its blob **through the R1412 validated
    /// `Deserialize`** and, only if it is a valid topology, swap the live
    /// `Signal`. A blob that violates a `try_new` invariant (the `"corrupt"`
    /// witness) is a deserialize error: the status line records the rejection
    /// and the live topology is left untouched.
    fn apply(&self, name: &str) -> Result<(), String> {
        let Some(blob) = self.stored_blob(name) else {
            let msg = format!("no preset '{name}'");
            self.status.set(msg.clone());
            return Err(msg);
        };
        match serde_json::from_str::<DockTopology>(&blob) {
            Ok(topo) => {
                self.topology.set(Some(topo));
                self.active.set(name.to_owned());
                self.status.set(format!("applied '{name}'"));
                Ok(())
            }
            Err(e) => {
                let msg = format!("rejected '{name}': {e}");
                self.status.set(msg.clone());
                Err(msg)
            }
        }
    }

    /// Save the live topology under `name` (overwriting a same-named preset).
    fn save(&self, name: &str) -> Result<(), String> {
        let Some(topo) = self.topology.get() else {
            let msg = "cannot save an empty dock".to_owned();
            self.status.set(msg.clone());
            return Err(msg);
        };
        let blob = serialize_topology(&topo);
        let mut presets = self.presets.get();
        if let Some(slot) = presets.iter_mut().find(|(n, _)| *n == name) {
            slot.1 = blob;
        } else {
            presets.push((name.to_owned(), blob));
        }
        self.presets.set(presets);
        self.status.set(format!("saved '{name}'"));
        Ok(())
    }

    /// Delete a named preset. Deleting the active preset leaves the live
    /// topology in place (the layout you are looking at does not vanish).
    fn delete(&self, name: &str) -> Result<(), String> {
        let mut presets = self.presets.get();
        let before = presets.len();
        presets.retain(|(n, _)| n != name);
        if presets.len() == before {
            let msg = format!("no preset '{name}'");
            self.status.set(msg.clone());
            return Err(msg);
        }
        self.presets.set(presets);
        self.status.set(format!("deleted '{name}'"));
        Ok(())
    }
}

// ── the RPC command / introspection external ──────────────────────────────

#[derive(Debug)]
struct PresetCommandExternal {
    sig: PresetSignals,
}

impl std::fmt::Debug for PresetSignals {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PresetSignals").finish_non_exhaustive()
    }
}

impl External for PresetCommandExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Rpc], BackendFallback::Skip)
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
}

impl ExternalIntrospect for PresetCommandExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    // The stored preset names, in order (a JSON array of strings).
                    SchemaField::new("names", "json"),
                    // The name of the currently-applied preset.
                    SchemaField::new("active", "text"),
                    // The last command's outcome (e.g. "applied 'wide'",
                    // "rejected 'corrupt': ...").
                    SchemaField::new("status", "text"),
                    // The stored preset count.
                    SchemaField::new("count", "int"),
                    // The live topology as a serialized blob — proof of what is
                    // actually applied, independent of the painted rects.
                    SchemaField::new("active_blob", "text"),
                    // Command surface: `apply` / `save` / `delete` a named preset.
                    SchemaField::new("apply", "string"),
                    SchemaField::new("save", "string"),
                    SchemaField::new("delete", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        match path {
            "names" => {
                let names: Vec<String> =
                    self.sig.presets.get().into_iter().map(|(n, _)| n).collect();
                Some(IntrospectValue::Json(serde_json::json!(names)))
            }
            "active" => Some(IntrospectValue::Text(self.sig.active.get())),
            "status" => Some(IntrospectValue::Text(self.sig.status.get())),
            "count" => Some(IntrospectValue::Int(
                i64::try_from(self.sig.presets.get().len()).unwrap_or(i64::MAX),
            )),
            "active_blob" => self
                .sig
                .topology
                .get()
                .map(|t| IntrospectValue::Text(serialize_topology(&t))),
            _ => None,
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        // Every field is read-only or a command (via `invoke`); nothing is a
        // directly-settable value.
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Text(name) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let result = match path {
            "apply" => self.sig.apply(&name),
            "save" => self.sig.save(&name),
            "delete" => self.sig.delete(&name),
            _ => return Err(InvokeError::UnknownPath),
        };
        // Report the resulting status either way; a rejected apply is a normal
        // outcome (the corrupt-blob witness), not an RPC error.
        Ok(IntrospectValue::Text(match result {
            Ok(()) => self.sig.status.get(),
            Err(msg) => msg,
        }))
    }
}

// ── the passive window-root external (primary) ────────────────────────────

/// A do-nothing primary external. The window needs a primary input target, but
/// this demo's interaction is entirely RPC-driven through the
/// [`PresetCommandExternal`] extra, so the root surface carries no behaviour.
#[derive(Debug, Default)]
struct RootExternal;

impl External for RootExternal {
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }
}

// ── view ──────────────────────────────────────────────────────────────────

/// A panel body: a label naming the panel. The dock walker wraps this in a
/// chrome frame tagged with the `panel_id`, so `find_by_tag(snap, panel_id)`
/// resolves the panel's measured rect.
fn panel_content(panel_id: &str, theme: &Theme) -> Scene {
    let fg = theme.resolve(ColorRole::OnSurface);
    Scene::Text(
        TextNode::styled(
            panel_id.to_owned(),
            Rect::default(),
            TextStyle::new().with_size_px(STATUS_FONT_PX).with_fg(fg),
        )
        .with_layout(LayoutStyle::new().with_absolute_position(10, 10)),
    )
}

/// The dock surface for the current topology, given the flex-main treatment so
/// it fills the column below the preset bar (the R1206 flex-main idiom: basis 0
/// / grow 1 / min-height 0).
fn view_dock(theme: &Theme) -> Scene {
    let dock = match use_topology().get() {
        Some(topology) => view_dock_surface(
            &topology,
            |panel_id| panel_content(panel_id, theme),
            |split_id, initial_ratio| DockSplitState {
                ratio_signal: use_split_ratio(split_id.to_owned(), initial_ratio),
                dragging: false,
            },
            |_| None,
            theme,
        ),
        None => Scene::Container(ContainerNode::new(vec![])),
    };
    match dock {
        Scene::Container(mut c) => {
            c.layout = c
                .layout
                .with_flex_basis(SizeValue::Px(0))
                .with_flex_grow(1.0)
                .with_min_size(Size::auto().with_height(SizeValue::Px(0)));
            Scene::Container(c)
        }
        other => other,
    }
}

/// The fixed-height preset bar: a status line + an "active | presets: …" line,
/// each a tagged `TextNode` so the demo reads them back as scene data.
fn view_preset_bar(theme: &Theme) -> Scene {
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);

    let names: Vec<String> = use_presets().get().into_iter().map(|(n, _)| n).collect();
    let active = use_active().get();
    let status = use_status().get();

    let status_line = Scene::Text(
        TextNode::styled(
            format!("dock preset manager  |  {status}"),
            Rect::default(),
            TextStyle::new()
                .with_size_px(TITLE_FONT_PX)
                .with_fg(on_surface),
        )
        .with_tag(STATUS_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(12, 10)),
    );

    let active_line = Scene::Text(
        TextNode::styled(
            format!("active: {active}   presets: {}", names.join(", ")),
            Rect::default(),
            TextStyle::new()
                .with_size_px(STATUS_FONT_PX)
                .with_fg(on_surface_muted),
        )
        .with_tag(ACTIVE_TAG)
        .with_layout(LayoutStyle::new().with_absolute_position(12, 32)),
    );

    Scene::Container(
        ContainerNode::new(vec![status_line, active_line])
            .with_style(BoxStyle::filled(
                theme.resolve(ColorRole::SurfaceContainerLow),
            ))
            .with_layout(
                LayoutStyle::new()
                    .with_flex_basis(SizeValue::Px(BAR_H))
                    .with_size(Size::auto().with_height(SizeValue::Px(BAR_H))),
            ),
    )
}

/// view-fn (§6.3): pure sync mapping. State is `()` — every dynamic value is a
/// reactive `Signal` read here (topology, presets, active, status), so an RPC
/// command's `Signal::set` repaints the surface.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "the WidgetCore::view trait hands the frame by reference; the signature mirrors it"
)]
fn view(_state: (), _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let surface = theme.resolve(ColorRole::Surface);
    Scene::Container(
        ContainerNode::new(vec![view_preset_bar(&theme), view_dock(&theme)])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Stretch)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

// ── the binding ────────────────────────────────────────────────────────────

struct PresetsView;

impl WidgetCore for PresetsView {
    type State = ();
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(RootExternal)
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Runs in the root owner, so `use_*` here share their `Signal` slots
        // with the view — a command's `Signal::set` reaches the next paint.
        let sig = PresetSignals {
            topology: use_topology(),
            presets: use_presets(),
            active: use_active(),
            status: use_status(),
        };
        vec![ExtraExternal::new(
            PRESETS_TAG,
            Box::new(PresetCommandExternal { sig }),
        )]
    }

    fn tag() -> &'static str {
        ROOT_TAG
    }

    fn read_state(_scene: &Scene) {}

    fn view(state: (), frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-dock-presets (R1412 §5.49 dock-layout preset manager)"
    }

    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }
}

impl WidgetA11y for PresetsView {
    fn access_node(_state: &(), _focused: Option<&str>) -> Vec<AccessNode> {
        let names: Vec<String> = use_presets().get().into_iter().map(|(n, _)| n).collect();
        vec![
            AccessNode::new(PRESETS_TAG, AriaRole::Group)
                .with_name("Dock layout preset manager")
                .with_value(AccessValue::Text(format!(
                    "active {}, presets: {}",
                    use_active().get(),
                    names.join(", ")
                ))),
        ]
    }
}

impl WidgetView for PresetsView {
    type Renderer = HelloDockPresetsRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<PresetsView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signals() -> PresetSignals {
        PresetSignals {
            topology: use_topology(),
            presets: use_presets(),
            active: use_active(),
            status: use_status(),
        }
    }

    #[test]
    fn the_builtin_presets_are_distinct_topologies() {
        Owner::new().run(|| {
            let wide = build_wide();
            let tall = build_tall();
            let editor = build_editor();
            assert_ne!(wide, tall, "wide and tall differ");
            assert_ne!(wide, editor, "wide and editor differ");
            assert_ne!(tall, editor, "tall and editor differ");
            // All three arrange the same panel set.
            let mut expected = PANELS.to_vec();
            expected.sort_unstable();
            for topo in [&wide, &tall, &editor] {
                let mut ids = topo.panel_ids();
                ids.sort_unstable();
                assert_eq!(ids, expected);
            }
        });
    }

    #[test]
    fn applying_a_valid_preset_swaps_the_live_topology() {
        Owner::new().run(|| {
            let sig = signals();
            assert_eq!(sig.active.get(), "editor", "boot is the editor layout");
            sig.apply("wide").expect("wide is a valid preset");
            assert_eq!(sig.active.get(), "wide");
            assert_eq!(sig.topology.get(), Some(build_wide()));
            assert_eq!(sig.status.get(), "applied 'wide'");
        });
    }

    #[test]
    fn applying_the_corrupt_preset_is_rejected_and_leaves_the_topology_intact() {
        Owner::new().run(|| {
            let sig = signals();
            sig.apply("wide").expect("set a known live layout first");
            let live_before = sig.topology.get();
            // THE R1412 SEAM: the corrupt blob (duplicate panel id) fails the
            // validated Deserialize, so apply errors and nothing is applied.
            let err = sig.apply("corrupt").expect_err("corrupt blob is rejected");
            assert!(
                err.contains("rejected 'corrupt'"),
                "status names the rejection: {err}"
            );
            assert_eq!(
                sig.topology.get(),
                live_before,
                "a rejected apply must not change the live topology"
            );
            assert_eq!(sig.active.get(), "wide", "the active preset is unchanged");
        });
    }

    #[test]
    fn save_then_apply_round_trips_through_serde() {
        Owner::new().run(|| {
            let sig = signals();
            sig.apply("tall").expect("apply tall");
            sig.save("my-layout").expect("save the live layout");
            assert!(
                sig.presets.get().iter().any(|(n, _)| n == "my-layout"),
                "the saved preset appears in the store"
            );
            // Switch away, then apply the saved copy — it must reconstruct the
            // tall topology through the validated Deserialize.
            sig.apply("wide").expect("apply wide");
            sig.apply("my-layout").expect("apply the saved copy");
            assert_eq!(
                sig.topology.get(),
                Some(build_tall()),
                "round-trips to tall"
            );
        });
    }

    #[test]
    fn deleting_a_preset_removes_it_but_keeps_the_live_layout() {
        Owner::new().run(|| {
            let sig = signals();
            sig.apply("wide").expect("apply wide");
            let live = sig.topology.get();
            sig.delete("wide").expect("delete the wide preset");
            assert!(
                !sig.presets.get().iter().any(|(n, _)| n == "wide"),
                "wide is gone from the store"
            );
            assert_eq!(
                sig.topology.get(),
                live,
                "the live layout survives its preset's deletion"
            );
            assert!(
                sig.delete("nope").is_err(),
                "deleting a missing preset errors"
            );
        });
    }
}
