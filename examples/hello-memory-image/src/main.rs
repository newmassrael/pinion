// R1404 §5.16 — example bindings tolerate looser doc-markdown lints.
#![allow(clippy::doc_markdown)]

//! `hello-memory-image` — R1404 §5.16 — a **producer-supplied in-memory image
//! source**: a [`Scene::Image`] whose `source` is `memory://<key>`, drawn from
//! an RGBA buffer a producer registered at runtime with **no filesystem**.
//!
//! ## What this proves (the seam sprag needs)
//!
//! Before R1404, `Scene::Image`'s source was a filesystem path only
//! (`ImageCache::resolve` → `std::fs::read`). A producer that decodes an
//! image at runtime — a terminal's Kitty-graphics / sixel raster, an
//! app-generated bitmap — had no way to hand pinion those pixels. R1404 adds
//! the [`MemoryImageStore`]: the shell seeds one at root, hands the handle to
//! every window's `ImageCache`, and a producer registers decoded RGBA under
//! a key. A `Scene::Image { source: "memory://<key>" }` node then paints it,
//! GPU-backed and headless alike, with no file round-trip.
//!
//! The store is **mutable**, which terminal images require (Kitty animation /
//! retransmit / delete). This binding's [`MemoryImageOracle`] registers a
//! procedurally-generated four-quadrant image under `memory://tile` and, on
//! command, **swaps** it for the other palette, **removes** it, and
//! **restores** it. The painter re-resolves the store every frame, so the
//! picture changes colour and disappears live — no scene edit, only the
//! store's pixels change. Four distinct quadrant colours (not a solid fill)
//! make the change decisive to a live-pixel check and to the eye.
//!
//! ## The AI-first witness (§2 #7 scene-as-data)
//!
//! `scene/snapshot` reports the `Scene::Image` node with its
//! `source: "memory://tile"` (an AI sees the memory scheme in the scene). The
//! primary [`MemoryImageOracle`] then exposes the store state a snapshot
//! cannot — whether the key is currently registered and which palette — so a
//! client drives + verifies the mutation with no pixel: `scene/intervene
//! /external/variant "warm"` (swap palette) or `/external/present false`
//! (remove), `scene/invoke /external/send "swap"` (the router send channel),
//! reading it back at `variant` / `present` / `registered` / `width`. See
//! `tools/demos/r1404_memory_image.py`.

use pinion_a11y::{AccessNode, AccessValue, AriaRole, WidgetA11y};
use pinion_core::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, InvokeError, RepaintOwner, SchemaField, ThreadOwnership,
};
use pinion_core::scene::{BoxNode, ContainerNode, ImageNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, ImageStyle, JustifyContent, LayoutStyle, Size,
    TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_runtime::{DecodedImage, MemoryImageStore, use_image_store};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloMemoryImageRenderer, HelloMemoryImageRendererError);

const WIN_W: u32 = 360;
const WIN_H: u32 = 320;
const THEME_TAG: &str = "app";

/// The grid's paint tag **and** the primary [`MemoryImageOracle`]'s
/// registration tag — the oracle is addressed over RPC as `/external/<field>`.
const ORACLE_TAG: &str = "memory_image";

/// The producer store key the image is registered under. The `Scene::Image`
/// source is the `MEMORY_SCHEME` joined with this — see [`IMAGE_SOURCE`].
const MEMORY_KEY: &str = "tile";

/// The `Scene::Image` source: `memory://tile`. Kept as one literal (the
/// `strip_prefix` in `ImageCache::resolve` parses this exact shape); a test
/// pins it to `MEMORY_SCHEME` + `MEMORY_KEY`.
const IMAGE_SOURCE: &str = "memory://tile";

/// The registered image's side, in pixels (an NxN four-quadrant bitmap). Small
/// on purpose: this is a decoded RGBA buffer the producer hands over, not a
/// file, and `Fit::Fill` stretches it to the cell.
const IMG_SIDE: u32 = 16;

/// The bordered cell the image fills.
const CELL: u32 = 200;

const TITLE_FONT_PX: u32 = 18;
const STATUS_FONT_PX: u32 = 12;

// --- The registered image --------------------------------------------------

/// Which of the two palettes is registered. Toggling it (a re-register under
/// the same key) is the mutable-image update a terminal needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Palette {
    Cool,
    Warm,
}

impl Palette {
    /// The four quadrant colours (top-left, top-right, bottom-left,
    /// bottom-right). Distinct hues so a solid fill cannot fake the image.
    fn quadrants(self) -> [Color; 4] {
        match self {
            Palette::Cool => [
                Color::rgb(0x21, 0x5b, 0xd0), // blue
                Color::rgb(0x1f, 0xa8, 0xc0), // cyan
                Color::rgb(0x2e, 0xa0, 0x4a), // green
                Color::rgb(0x16, 0x6b, 0x5e), // teal
            ],
            Palette::Warm => [
                Color::rgb(0xd0, 0x33, 0x33), // red
                Color::rgb(0xe0, 0x8a, 0x1e), // orange
                Color::rgb(0xd8, 0xc4, 0x1e), // yellow
                Color::rgb(0xb0, 0x36, 0x9a), // magenta
            ],
        }
    }

    /// The palette's lowercase name (the `variant` introspect value).
    fn name(self) -> &'static str {
        match self {
            Palette::Cool => "cool",
            Palette::Warm => "warm",
        }
    }

    /// The other palette (the `swap` target).
    fn other(self) -> Palette {
        match self {
            Palette::Cool => Palette::Warm,
            Palette::Warm => Palette::Cool,
        }
    }

    /// Parse a palette name (`"cool"` / `"warm"`).
    fn from_name(s: &str) -> Option<Palette> {
        match s.trim() {
            "cool" => Some(Palette::Cool),
            "warm" => Some(Palette::Warm),
            _ => None,
        }
    }
}

/// Build the `IMG_SIDE`x`IMG_SIDE` four-quadrant RGBA image for `palette`
/// (opaque). This is the "producer decoded an image at runtime" — a plain
/// [`DecodedImage`], no file, no PNG bytes.
fn quad_image(palette: Palette) -> DecodedImage {
    let colors = palette.quadrants();
    let half = IMG_SIDE / 2;
    let mut px = Vec::with_capacity((IMG_SIDE * IMG_SIDE * 4) as usize);
    for y in 0..IMG_SIDE {
        for x in 0..IMG_SIDE {
            // quadrant index: 0=TL, 1=TR, 2=BL, 3=BR.
            let q = usize::from(x >= half) + usize::from(y >= half) * 2;
            let c = colors[q];
            px.extend_from_slice(&[c.r, c.g, c.b, 0xff]);
        }
    }
    DecodedImage::from_rgba8(IMG_SIDE, IMG_SIDE, px).expect("a full opaque RGBA buffer")
}

// --- The view --------------------------------------------------------------

/// view-fn (§6.3): pure sync mapping. `present` is whether the store currently
/// holds `memory://tile`; `palette` is the last-registered palette (for the
/// caption). The `Scene::Image` is emitted unconditionally — when the store
/// has no `tile`, the painter's resolve returns `None` and it paints nothing
/// (the graceful missing-source skip), so the cell shows through.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "mirrors the WidgetCore::view(&Frame) signature the caller forwards"
)]
fn view(present: bool, palette: Palette, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let on_surface_muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let surface = theme.resolve(ColorRole::Surface);

    let title = Scene::Text(TextNode::styled(
        "Image — memory://tile (producer-registered)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(TITLE_FONT_PX)
            .with_fg(on_surface),
    ));

    // The bordered cell, with the memory-sourced image absolutely positioned
    // to fill it (the hello-image stack pattern).
    let cell_style = BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerHigh))
        .with_corner_radius(8)
        .with_border(pinion_core::style::Border::new(
            theme.resolve(ColorRole::Outline),
            1,
        ));
    let frame = Scene::Box(
        BoxNode::new(Rect::default(), cell_style)
            .with_layout(LayoutStyle::new().with_size(Size::px(CELL, CELL))),
    );
    let image = Scene::Image(
        ImageNode::styled(
            IMAGE_SOURCE,
            Rect::default(),
            ImageStyle::default().with_fit(pinion_core::style::Fit::Fill),
        )
        .with_tag("tile_image")
        .with_layout(
            LayoutStyle::new()
                .with_absolute_position(0, 0)
                .with_size(Size::px(CELL, CELL)),
        ),
    );
    let stack = Scene::Container(
        ContainerNode::new(vec![frame, image])
            .with_layout(LayoutStyle::new().with_size(Size::px(CELL, CELL))),
    );

    let status_text = if present {
        format!(
            "source {IMAGE_SOURCE} | palette {} | {IMG_SIDE}x{IMG_SIDE} registered",
            palette.name()
        )
    } else {
        format!("source {IMAGE_SOURCE} | absent (removed — nothing paints)")
    };
    let status = Scene::Text(TextNode::styled(
        status_text,
        Rect::default(),
        TextStyle::new()
            .with_size_px(STATUS_FONT_PX)
            .with_fg(on_surface_muted),
    ));

    Scene::Container(
        ContainerNode::new(vec![title, stack, status])
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_gap(16),
            ),
    )
}

/// Read `(present, palette)` from the primary [`MemoryImageOracle`] in the
/// state scene; the boot default when the external is absent (a bare view-fn
/// unit test).
fn read_oracle(scene: &Scene) -> (bool, Palette) {
    let Some(intro) = scene
        .find_external_with_tag(ORACLE_TAG)
        .and_then(|n| n.handle.introspect())
    else {
        return (true, Palette::Cool);
    };
    let present = matches!(intro.query("present"), Some(IntrospectValue::Bool(true)));
    let palette = match intro.query("variant") {
        Some(IntrospectValue::Text(name)) => Palette::from_name(&name).unwrap_or(Palette::Cool),
        _ => Palette::Cool,
    };
    (present, palette)
}

// --- The producer oracle (primary external) --------------------------------

/// Owns the producer [`MemoryImageStore`] handle and the current registration.
/// The read half is an introspectable oracle (`image_source` / `variant` /
/// `present` / `registered` / `width` / `height`) so an AI client sees the
/// store state a snapshot cannot. The write half MUTATES the store: `swap`
/// re-registers the other palette, `remove` deletes the key, `restore`
/// re-registers — the mutable-image gestures a terminal image needs, driven
/// by `intervene variant` / `intervene present` (AI-first, no pixel) or
/// `invoke send`.
#[derive(Debug, Clone)]
struct MemoryImageOracle {
    /// The shared producer store the painter resolves `memory://tile` through.
    store: MemoryImageStore,
    /// The last-registered palette (kept even while removed, so `restore`
    /// re-registers the same one).
    palette: Palette,
    /// Whether `memory://tile` is currently registered.
    present: bool,
}

impl MemoryImageOracle {
    /// Resolve the shared store (an active `Owner` scope — `create_external`
    /// runs inside one) and register the boot image under `memory://tile`.
    fn new() -> Self {
        let store = use_image_store();
        let palette = Palette::Cool;
        store.insert(MEMORY_KEY, &quad_image(palette));
        Self {
            store,
            palette,
            present: true,
        }
    }

    /// Register (or re-register) `palette` under the key; a subsequent paint
    /// draws it.
    fn register(&mut self, palette: Palette) {
        self.store.insert(MEMORY_KEY, &quad_image(palette));
        self.palette = palette;
        self.present = true;
    }

    /// Remove the key; a subsequent paint draws nothing.
    fn remove(&mut self) {
        self.store.remove(MEMORY_KEY);
        self.present = false;
    }
}

impl External for MemoryImageOracle {
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
}

impl ExternalIntrospect for MemoryImageOracle {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("image_source", "string"),
                    SchemaField::new("image_key", "string"),
                    SchemaField::new("variant", "string"),
                    SchemaField::new("present", "bool"),
                    SchemaField::new("registered", "int"),
                    SchemaField::new("width", "int"),
                    SchemaField::new("height", "int"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let int = |n: usize| IntrospectValue::Int(i64::try_from(n).unwrap_or(0));
        match path {
            "image_source" => Some(IntrospectValue::Text(IMAGE_SOURCE.to_owned())),
            "image_key" => Some(IntrospectValue::Text(MEMORY_KEY.to_owned())),
            "variant" => Some(IntrospectValue::Text(self.palette.name().to_owned())),
            "present" => Some(IntrospectValue::Bool(self.present)),
            // The store holds only this key, so this is 0 or 1 — the painter's
            // view of "is there a pixel source" as a count.
            "registered" => Some(int(self.store.len())),
            "width" | "height" => Some(IntrospectValue::Int(if self.present {
                i64::from(IMG_SIDE)
            } else {
                0
            })),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            // AI-first, no-pixel palette swap: a name string re-registers that
            // palette (and makes it present).
            "variant" => match value {
                IntrospectValue::Text(ref s) => {
                    let p = Palette::from_name(s).ok_or(InterveneError::TypeMismatch)?;
                    self.register(p);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            // AI-first, no-pixel remove / restore: `false` deletes the key,
            // `true` re-registers the current palette.
            "present" => match value {
                IntrospectValue::Bool(true) => {
                    self.register(self.palette);
                    Ok(())
                }
                IntrospectValue::Bool(false) => {
                    self.remove();
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "image_source" | "image_key" | "registered" | "width" | "height" => {
                Err(InterveneError::ReadOnly)
            }
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // The router send channel + the RPC mutation verb. Each command
            // mutates the store and returns the resulting state as text so the
            // caller sees the effect without a follow-up query.
            "send" => {
                let IntrospectValue::Text(ref cmd) = args else {
                    return Err(InvokeError::TypeMismatch);
                };
                let out = match cmd.trim() {
                    "swap" => {
                        self.register(self.palette.other());
                        self.palette.name()
                    }
                    "restore" => {
                        self.register(self.palette);
                        self.palette.name()
                    }
                    "cool" => {
                        self.register(Palette::Cool);
                        "cool"
                    }
                    "warm" => {
                        self.register(Palette::Warm);
                        "warm"
                    }
                    "remove" => {
                        self.remove();
                        "absent"
                    }
                    // An unknown send is ignored (the hex-dump send discipline).
                    _ => return Ok(IntrospectValue::Null),
                };
                Ok(IntrospectValue::Text(out.to_owned()))
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// --- The binding -----------------------------------------------------------

/// The binding. The [`MemoryImageOracle`] is the root external (the producer
/// store owner + mutation surface); a manual [`WidgetCore`] — the image is
/// RPC-driven with no keyboard channel.
struct MemoryImageView;

impl WidgetCore for MemoryImageView {
    /// `(present, palette)`, read from the primary oracle.
    type State = (bool, Palette);
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(MemoryImageOracle::new())
    }

    fn tag() -> &'static str {
        ORACLE_TAG
    }

    fn read_state(scene: &Scene) -> (bool, Palette) {
        read_oracle(scene)
    }

    fn view(state: (bool, Palette), frame: &Frame) -> Scene {
        view(state.0, state.1, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-memory-image (R1404 §5.16 producer memory:// image source)"
    }

    /// RPC-driven (`intervene` / `invoke send`); no keyboard channel.
    fn apply_key(
        _scene: &mut Scene,
        _focused: Option<&str>,
        _key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        false
    }

    fn fmt_state_log(state: &(bool, Palette)) -> String {
        format!("present {} / palette {}", state.0, state.1.name())
    }
}

impl WidgetA11y for MemoryImageView {
    /// The image as an `Image` node whose value states the current palette or
    /// that nothing is registered.
    fn access_node(state: &(bool, Palette), _focused: Option<&str>) -> Vec<AccessNode> {
        let (present, palette) = *state;
        let value = if present {
            format!("{} palette image from memory://tile", palette.name())
        } else {
            "no image registered".to_owned()
        };
        vec![
            AccessNode::new(ORACLE_TAG, AriaRole::Group)
                .with_name("Producer memory image")
                .with_value(AccessValue::Text(value)),
        ]
    }
}

impl WidgetView for MemoryImageView {
    type Renderer = HelloMemoryImageRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<MemoryImageView>();
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_runtime::MEMORY_SCHEME;

    #[test]
    fn image_source_is_the_memory_scheme_plus_key() {
        assert!(IMAGE_SOURCE.starts_with(MEMORY_SCHEME));
        assert_eq!(IMAGE_SOURCE.strip_prefix(MEMORY_SCHEME), Some(MEMORY_KEY));
    }

    #[test]
    fn quad_image_is_a_full_opaque_rgba_buffer_with_four_colours() {
        let img = quad_image(Palette::Cool);
        assert_eq!((img.width(), img.height()), (IMG_SIDE, IMG_SIDE));
        assert_eq!(img.pixels().len(), (IMG_SIDE * IMG_SIDE * 4) as usize);
        // Every alpha byte is opaque.
        assert!(img.pixels().chunks_exact(4).all(|p| p[3] == 0xff));
        // The four quadrant centres are the four declared colours.
        let side = IMG_SIDE as usize;
        let px = |x: usize, y: usize| {
            let i = (y * side + x) * 4;
            Color::rgb(img.pixels()[i], img.pixels()[i + 1], img.pixels()[i + 2])
        };
        let q = Palette::Cool.quadrants();
        assert_eq!(px(3, 3), q[0], "top-left quadrant");
        assert_eq!(px(12, 3), q[1], "top-right quadrant");
        assert_eq!(px(3, 12), q[2], "bottom-left quadrant");
        assert_eq!(px(12, 12), q[3], "bottom-right quadrant");
        // The two palettes differ (a swap is a visible change).
        assert_ne!(Palette::Cool.quadrants(), Palette::Warm.quadrants());
    }

    /// An oracle over a bare store (not the shell-seeded one) — the unit tests
    /// exercise the mutation surface without an `Owner`.
    fn oracle() -> MemoryImageOracle {
        let store = MemoryImageStore::new();
        store.insert(MEMORY_KEY, &quad_image(Palette::Cool));
        MemoryImageOracle {
            store,
            palette: Palette::Cool,
            present: true,
        }
    }

    #[test]
    fn oracle_reports_the_source_and_boot_state() {
        let o = oracle();
        assert_eq!(
            o.query("image_source"),
            Some(IntrospectValue::Text(IMAGE_SOURCE.into()))
        );
        assert_eq!(
            o.query("image_key"),
            Some(IntrospectValue::Text(MEMORY_KEY.into()))
        );
        assert_eq!(
            o.query("variant"),
            Some(IntrospectValue::Text("cool".into()))
        );
        assert_eq!(o.query("present"), Some(IntrospectValue::Bool(true)));
        assert_eq!(o.query("registered"), Some(IntrospectValue::Int(1)));
        assert_eq!(
            o.query("width"),
            Some(IntrospectValue::Int(i64::from(IMG_SIDE)))
        );
        assert_eq!(o.query("nope"), None);
    }

    #[test]
    fn invoke_send_swaps_removes_and_restores_and_mutates_the_store() {
        let mut o = oracle();
        assert!(o.store.contains(MEMORY_KEY));

        // swap -> the warm palette is now registered.
        assert_eq!(
            o.invoke("send", IntrospectValue::Text("swap".into())),
            Ok(IntrospectValue::Text("warm".into()))
        );
        assert_eq!(
            o.query("variant"),
            Some(IntrospectValue::Text("warm".into()))
        );
        assert!(
            o.store.contains(MEMORY_KEY),
            "still registered after a swap"
        );

        // remove -> the key is gone; a paint would draw nothing.
        assert_eq!(
            o.invoke("send", IntrospectValue::Text("remove".into())),
            Ok(IntrospectValue::Text("absent".into()))
        );
        assert_eq!(o.query("present"), Some(IntrospectValue::Bool(false)));
        assert_eq!(o.query("registered"), Some(IntrospectValue::Int(0)));
        assert_eq!(o.query("width"), Some(IntrospectValue::Int(0)));
        assert!(!o.store.contains(MEMORY_KEY), "removed from the store");

        // restore -> the last (warm) palette is registered again.
        assert_eq!(
            o.invoke("send", IntrospectValue::Text("restore".into())),
            Ok(IntrospectValue::Text("warm".into()))
        );
        assert_eq!(o.query("present"), Some(IntrospectValue::Bool(true)));
        assert!(o.store.contains(MEMORY_KEY));

        // An unknown send is ignored; a non-text arg is a type error.
        assert_eq!(
            o.invoke("send", IntrospectValue::Text("bogus".into())),
            Ok(IntrospectValue::Null)
        );
        assert_eq!(
            o.invoke("send", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch)
        );
        assert_eq!(
            o.invoke("nope", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        );
    }

    #[test]
    fn intervene_variant_and_present_drive_the_store_no_pixel() {
        let mut o = oracle();
        // Set the palette by name.
        o.intervene("variant", IntrospectValue::Text("warm".into()))
            .unwrap();
        assert_eq!(
            o.query("variant"),
            Some(IntrospectValue::Text("warm".into()))
        );
        // A bad name is a type error.
        assert_eq!(
            o.intervene("variant", IntrospectValue::Text("teal".into())),
            Err(InterveneError::TypeMismatch)
        );
        // present=false removes, present=true restores.
        o.intervene("present", IntrospectValue::Bool(false))
            .unwrap();
        assert!(!o.store.contains(MEMORY_KEY));
        o.intervene("present", IntrospectValue::Bool(true)).unwrap();
        assert!(o.store.contains(MEMORY_KEY));
        // Read-only + unknown slots are guarded.
        assert_eq!(
            o.intervene("image_source", IntrospectValue::Text("x".into())),
            Err(InterveneError::ReadOnly)
        );
        assert_eq!(
            o.intervene("nope", IntrospectValue::Null),
            Err(InterveneError::UnknownPath)
        );
    }

    /// The first `memory://` image node's source in a scene tree.
    fn find_image_source(scene: &Scene) -> Option<String> {
        match scene {
            Scene::Image(i) => Some(i.source.clone()),
            Scene::Container(c) => c.children.iter().find_map(find_image_source),
            _ => None,
        }
    }

    #[test]
    fn view_carries_the_memory_image_node() {
        // The image node with the memory:// source is present.
        let scene = pinion_core::Owner::new().run(|| view(true, Palette::Cool, &Frame::new()));
        assert_eq!(find_image_source(&scene).as_deref(), Some(IMAGE_SOURCE));
        // Even when absent the node is still emitted (paints nothing).
        let absent = pinion_core::Owner::new().run(|| view(false, Palette::Warm, &Frame::new()));
        assert_eq!(find_image_source(&absent).as_deref(), Some(IMAGE_SOURCE));
    }

    #[test]
    fn read_oracle_defaults_when_no_external() {
        // A bare paint scene (no external) reads the boot default.
        let scene = pinion_core::Owner::new().run(|| view(true, Palette::Cool, &Frame::new()));
        assert_eq!(read_oracle(&scene), (true, Palette::Cool));
    }

    #[test]
    fn emits_image_access_node() {
        let nodes = <MemoryImageView as WidgetA11y>::access_node(&(true, Palette::Cool), None);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].role, AriaRole::Group);
    }
}
