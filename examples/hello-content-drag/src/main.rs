//! `hello-content-drag` — R1753 §5.45 §5.35: the first client of
//! [`ContentDrag`], and a reproduction of the report that asked for it.
//!
//! ## The report
//!
//! A consumer's window is **430 x 932** — phone-shaped. Its set list holds
//! eighteen rows and nine fit on screen, and every row is also a tap target:
//! press one to correct a set and open its card. Measured there, a press-and-
//! drag on the list moved the offset by **0 px** and opened the pressed row
//! instead. The owner's words: *"if it is a list, shouldn't it drag?"*
//!
//! On a desktop with a wheel that is survivable. At this shape it is the wrong
//! affordance: the list is the only thing on screen and dragging it is the only
//! gesture anyone would try.
//!
//! ## What this binary shows
//!
//! The same shape, with the region declaring
//! [`ContentDrag::Grab`] — one
//! builder call on the [`ScrollNode`]. Both halves of the gesture are then
//! visible at once, which is the point: the affordance is added to a list whose
//! rows are already tap targets **without taking the taps away**.
//!
//! * **Drag anywhere on the list** — press, move past the framework's
//!   click-vs-drag threshold, and the content follows the pointer from the
//!   press point. The row under the press is told the gesture left it
//!   (`PointerCancel`) and never opens.
//! * **Tap a row** — press and release without straying, and the row opens
//!   exactly as before.
//! * **Drag the empty strip below the last row** — no widget is under the
//!   press at all, and the list still pans.
//!
//! ## AI-first witness (§2 #7)
//!
//! The header band is not decoration — it is the demo's own evidence, printed
//! from the state an agent reads over the wire. `scene/snapshot` reports the
//! header text plus eighteen `set_list#<i>` rows, and the primary External
//! answers three slots that separate the two outcomes a press can have:
//!
//! | slot | meaning |
//! |---|---|
//! | `opened` | the row a completed TAP opened, or `Null` |
//! | `pressed` | the row currently holding a press, or `Null` |
//! | `cancels` | how many presses the pan channel took over |
//!
//! So a drag is witnessed as `cancels` rising while `opened` stays `Null`, and
//! a tap as `opened` landing with `cancels` unchanged. Those are different
//! observations rather than the same one read twice, which is what lets this
//! demo tell the fixed behaviour from the reported one.

use pinion_a11y::{AccessNode, WidgetA11y};
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    ReadRefusal, SchemaField,
};
use pinion_core::scene::{ContainerNode, Rect, Scene, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::widgets::scroll::{ContentDrag, use_scroll_state};
use pinion_core::{Frame, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, vello_renderer_impl};

// pinion-forge codegen output: `pub struct HelloContentDragRenderer` + error +
// async `new` + sync `render` / `resize`.
include!(concat!(env!("OUT_DIR"), "/app.rs"));

// Bridge the codegen renderer into `pinion_shell::VelloRenderer`.
vello_renderer_impl!(HelloContentDragRenderer, HelloContentDragRendererError);

/// The reported window, to the pixel. Not rounded to something tidier: the
/// whole argument is that this shape is where a wheel-only list stops being
/// acceptable, so the demo stands at that shape rather than near it.
const WIN_W: u32 = 430;
const WIN_H: u32 = 932;

/// Eighteen rows — a full day in the consumer's app.
const N: usize = 18;

const ROW_H: u32 = 56;
const ROW_GAP: u32 = 1;
const HEADER_H: u32 = 96;

/// The list viewport: the window minus the header.
///
/// How many rows that holds is NOT written here. A count in a comment is wrong
/// the moment a constant above it moves, and this one already was — the first
/// draft said "nine fit", borrowed from the consumer's app, whose rows are
/// taller than these. `r1753_the_list_overflows_its_viewport` derives it
/// instead, and fails if the geometry stops overflowing at all.
const VIEWPORT_H: u32 = WIN_H - HEADER_H;

const PRIMARY_TAG: &str = "set_list";
/// The scroll container's tag, not merely an owner-cache key: `use_scroll_state`
/// stamps it onto the `ScrollState`, and `ScrollNode::from_state` derives the
/// node's tag from it. So this is the name an agent addresses the viewport by
/// (`scene/snapshot` → `offset_y`), which is why it is spelled as a tag rather
/// than as a module path.
const SCROLL_KEY: &str = "set_list_scroll";

/// What the view needs to paint, read back from the External each frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ListState {
    /// The row a completed tap opened.
    opened: Option<usize>,
    /// The row currently under a press that has not yet been resolved.
    pressed: Option<usize>,
    /// How many presses the pan channel took over mid-gesture.
    cancels: u32,
}

/// The composite External behind every `set_list#<i>` row.
///
/// It holds exactly the three facts that distinguish a tap from a drag, and
/// nothing else. In particular it does **not** track the scroll offset: that
/// belongs to the `ScrollState` the framework owns, and a second copy here
/// would be a second answer to a question that already has one.
#[derive(Debug, Default)]
struct SetListExternal {
    opened: Option<usize>,
    pressed: Option<usize>,
    cancels: u32,
}

impl External for SetListExternal {
    fn backends(&self) -> pinion_core::external::BackendSupport {
        pinion_core::external::BackendSupport::new(
            &[pinion_core::external::Backend::Gui],
            pinion_core::external::BackendFallback::Skip,
        )
    }
    fn repaint_ownership(&self) -> pinion_core::external::RepaintOwner {
        pinion_core::external::RepaintOwner::Framework
    }
    fn thread_ownership(&self) -> pinion_core::external::ThreadOwnership {
        pinion_core::external::ThreadOwnership::UiThreadSync
    }
    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }
    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl ExternalIntrospect for SetListExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("opened", "int"),
                    SchemaField::new("pressed", "int"),
                    SchemaField::new("cancels", "int"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        let index = |v: Option<usize>| {
            v.and_then(|i| i64::try_from(i).ok())
                .map_or(IntrospectValue::Null, IntrospectValue::Int)
        };
        match path {
            "opened" => Ok(index(self.opened)),
            "pressed" => Ok(index(self.pressed)),
            "cancels" => Ok(IntrospectValue::Int(i64::from(self.cancels))),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        method: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        if method != "send" {
            return Ok(IntrospectValue::Null);
        }
        let IntrospectValue::Text(payload) = args else {
            return Ok(IntrospectValue::Null);
        };
        // The composite wire is `{row}:{Event}[:mods[:buttons]]`, decoded
        // through the grammar's own splitter rather than by slicing here.
        let Some(sent) = pinion_core::composite_tag::split_send_payload(&payload) else {
            return Ok(IntrospectValue::Null);
        };
        let Ok(row) = sent.key.parse::<usize>() else {
            return Ok(IntrospectValue::Null);
        };
        match sent.event {
            "PointerDown" => self.pressed = Some(row),
            // ★ The report's second half, and the consumer already fixed their
            // side of it: opening on the PRESS is what made a drag open a card.
            // A row opens on the release, which is also the only edge that can
            // tell a tap from a drag — a press cannot know yet.
            "PointerUp" => {
                self.pressed = None;
                self.opened = Some(row);
            }
            // ★ The pan channel took this gesture over. The row must un-press,
            // and must NOT open: the finger was scrolling.
            "PointerCancel" => {
                self.pressed = None;
                self.cancels += 1;
            }
            "PointerLeave" => self.pressed = None,
            _ => {}
        }
        Ok(IntrospectValue::Null)
    }
}

fn row_label(index: usize) -> String {
    format!("Set {}", index + 1)
}

/// One list row: a tap target, tagged `set_list#<i>` so the router's sub-index
/// split routes a hit on row `i` to the single composite External.
fn set_row(index: usize, state: ListState) -> Scene {
    let opened = state.opened == Some(index);
    let pressed = state.pressed == Some(index);
    let fill = if opened {
        Color::rgb(0x2f, 0x5d, 0x8f)
    } else if pressed {
        Color::rgb(0x33, 0x38, 0x3f)
    } else {
        Color::rgb(0x24, 0x27, 0x2c)
    };
    let label = Scene::Text(TextNode::styled(
        row_label(index),
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(Color::rgb(0xe6, 0xe8, 0xea)),
    ));
    Scene::Container(
        ContainerNode::new(vec![label])
            .with_tag(format!("{PRIMARY_TAG}#{index}"))
            .with_style(BoxStyle::filled(fill).with_corner_radius(6))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(WIN_W - 24, ROW_H))
                    .with_padding(Rect::new(16, 0, 16, 0)),
            ),
    )
}

/// The evidence band. Prints the three slots an agent reads, so a screenshot
/// and a `scene/snapshot` say the same thing.
fn header(state: ListState) -> Scene {
    let text = |s: String, px: u32, fg: Color| {
        Scene::Text(TextNode::styled(
            s,
            Rect::default(),
            TextStyle::new().with_size_px(px).with_fg(fg),
        ))
    };
    let shown = |v: Option<usize>| v.map_or_else(|| "-".to_owned(), row_label);
    Scene::Container(
        ContainerNode::new(vec![
            text(
                "drag the list to scroll  ·  tap a row to open".to_owned(),
                14,
                Color::rgb(0x9a, 0xa2, 0xac),
            ),
            text(
                format!(
                    "opened {}   pressed {}   cancels {}",
                    shown(state.opened),
                    shown(state.pressed),
                    state.cancels,
                ),
                16,
                Color::rgb(0xe6, 0xe8, 0xea),
            ),
        ])
        .with_tag("header")
        .with_style(BoxStyle::filled(Color::rgb(0x16, 0x18, 0x1c)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_justify(JustifyContent::Center)
                .with_gap(6)
                .with_size(Size::px(WIN_W, HEADER_H))
                .with_padding(Rect::new(16, 0, 16, 0)),
        ),
    )
}

// The `&Frame` is the `WidgetCore::view` signature this delegates from, so the
// reference is the trait's shape rather than this function's choice.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: ListState, _frame: &Frame) -> Scene {
    let scroll_state = use_scroll_state(SCROLL_KEY);
    let rows: Vec<Scene> = (0..N).map(|i| set_row(i, state)).collect();
    let content = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(ROW_GAP)
                .with_padding(Rect::new(12, 12, 12, 12)),
        ),
    );

    // ★★ R1753 — the whole opt-in, one call. Everything else in this file is
    // an ordinary list; what makes it draggable is this declaration, and its
    // absence is what the report was about.
    let scroll = ScrollNode::from_state(scroll_state, Rect::new(0, 0, WIN_W, VIEWPORT_H), content)
        .with_content_drag(ContentDrag::Grab);

    Scene::Container(
        ContainerNode::new(vec![header(state), Scene::Scroll(scroll)])
            .with_style(BoxStyle::filled(Color::rgb(0x0e, 0x0f, 0x12)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

struct ContentDragView;

impl WidgetCore for ContentDragView {
    type State = ListState;
    // Every state change arrives on the composite pointer wire, so the typed
    // keybinding slot stays unused.
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(SetListExternal::default())
    }

    fn tag() -> &'static str {
        PRIMARY_TAG
    }

    fn read_state(scene: &Scene) -> ListState {
        let mut out = ListState::default();
        let Some(node) = scene.find_external_with_tag(PRIMARY_TAG) else {
            return out;
        };
        let Some(intro) = node.handle.introspect() else {
            return out;
        };
        let index = |path: &str| match intro.query(path) {
            Ok(IntrospectValue::Int(i)) => usize::try_from(i).ok(),
            _ => None,
        };
        out.opened = index("opened");
        out.pressed = index("pressed");
        out.cancels = match intro.query("cancels") {
            Ok(IntrospectValue::Int(n)) => u32::try_from(n).unwrap_or(0),
            _ => 0,
        };
        out
    }

    fn view(state: ListState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion content-drag demo"
    }
}

impl WidgetA11y for ContentDragView {
    fn access_node(state: &ListState, focused: Option<&str>) -> Vec<AccessNode> {
        let mut nodes = Vec::with_capacity(N + 1);
        let list = (0..N).fold(
            AccessNode::new(PRIMARY_TAG, pinion_a11y::AriaRole::List).with_name("Sets"),
            |node, i| node.with_child(format!("{PRIMARY_TAG}#{i}")),
        );
        nodes.push(list);
        for i in 0..N {
            let tag = format!("{PRIMARY_TAG}#{i}");
            let selected = state.opened == Some(i);
            nodes.push(
                AccessNode::new(tag.clone(), pinion_a11y::AriaRole::ListItem)
                    .with_name(row_label(i))
                    .with_selected(selected)
                    .with_focused(focused == Some(tag.as_str())),
            );
        }
        nodes
    }
}

impl WidgetView for ContentDragView {
    type Renderer = HelloContentDragRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<ContentDragView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The declaration this whole binary exists to carry. Asserted on the
    /// painted scene rather than on the source, because what the router reads
    /// is the node — a builder call that failed to reach the tree would be
    /// invisible to any test that only read this file.
    #[test]
    fn r1753_the_list_declares_that_it_takes_content_drags() {
        let scene = pinion_core::reactive::Owner::new()
            .run(|| view(ListState::default(), &Frame::default()));
        let scroll = find_scroll(&scene).expect("the demo paints a scroll container");
        assert_eq!(
            scroll.content_drag,
            ContentDrag::Grab,
            "without this the list is the one in the report",
        );
        assert!(
            scroll.state.is_some(),
            "and the router needs a state to move"
        );
    }

    /// A press opens nothing; a release does. This is the consumer-side half of
    /// the report, kept here because the framework fix does not enforce it —
    /// a widget that still opened on `PointerDown` would open a card mid-drag
    /// no matter what the scroll container did with the motion.
    #[test]
    fn r1753_a_row_opens_on_the_release_and_not_on_the_press() {
        let mut ext = SetListExternal::default();
        let _ = ext.invoke("send", IntrospectValue::Text("3:PointerDown".to_owned()));
        assert_eq!(ext.opened, None, "a press is not yet a tap");
        assert_eq!(ext.pressed, Some(3));
        let _ = ext.invoke("send", IntrospectValue::Text("3:PointerUp".to_owned()));
        assert_eq!(ext.opened, Some(3));
        assert_eq!(ext.pressed, None);
    }

    /// And the escalation's cancel un-presses the row without opening it —
    /// the two outcomes a press can have, told apart by which edge arrives.
    #[test]
    fn r1753_a_cancelled_press_un_presses_without_opening() {
        let mut ext = SetListExternal::default();
        let _ = ext.invoke("send", IntrospectValue::Text("5:PointerDown".to_owned()));
        let _ = ext.invoke("send", IntrospectValue::Text("5:PointerCancel".to_owned()));
        assert_eq!(ext.pressed, None, "the row lets go");
        assert_eq!(ext.opened, None, "and the drag opened nothing");
        assert_eq!(ext.cancels, 1);
    }

    #[test]
    fn r1753_the_list_overflows_its_viewport() {
        // A list that fitted on screen would pass every test above while
        // proving nothing about scrolling.
        //
        // ★ It also OWNS the row count, which no comment does. R1753's closing
        // audit found "nine rows fit" written above `VIEWPORT_H` — a number
        // borrowed from the consumer's app, whose rows are taller, and false
        // of this one by five. A count stated in prose is wrong as soon as a
        // constant above it moves and nothing says so; a count derived here
        // moves with it, and the assertion that matters is the RELATION.
        let rows = u32::try_from(N).expect("N is a small literal");
        let pitch = ROW_H + ROW_GAP;
        let content_h = rows * pitch + 24;
        assert!(
            content_h > VIEWPORT_H,
            "content {content_h} must overflow viewport {VIEWPORT_H}",
        );
        let fit = (VIEWPORT_H - 12) / pitch;
        assert!(
            fit < rows,
            "{fit} of {rows} rows fit; a list that fits proves nothing",
        );
    }

    fn find_scroll(scene: &Scene) -> Option<&ScrollNode> {
        match scene {
            Scene::Scroll(s) => Some(s),
            Scene::Container(c) => c.children.iter().find_map(find_scroll),
            _ => None,
        }
    }
}
