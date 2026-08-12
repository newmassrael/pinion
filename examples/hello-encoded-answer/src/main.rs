//! `hello-encoded-answer` — R1480 §5.15 §5.7 §2 #2 §2 #7.
//!
//! A surface whose answer is *large* and whose encoding it already holds.
//!
//! `ExternalIntrospect::query` returns an [`IntrospectValue`], and until R1480
//! the only slot in that type able to carry a structured answer was
//! `Json(serde_json::Value)` — a DOM. So a producer holding a `Serialize`
//! type built a tree of maps and vectors, and the JSON-RPC envelope
//! immediately walked that tree to produce the text it was going to send.
//! Neither end wanted the tree: the producer had the data, the consumer gets
//! bytes. It existed because the channel's type demanded one.
//!
//! This binding serves an 80×24 terminal-style frame — the shape a display
//! client polls at interactive rates, and the case that reported the gap —
//! and answers it three ways so the difference is visible from outside:
//!
//!   * `frame` — [`IntrospectValue::raw`]: encoded once, spliced into the
//!     response frame verbatim.
//!   * `frame_via_dom` — [`IntrospectValue::json`]: the same document through
//!     the `Value` tree. Kept as this example's control, not as an alternative
//!     a real producer would offer: a binding picks one.
//!   * `encode` (invoke) — the same raw answer down the action channel, which
//!     is the other handler whose answer *is* the whole result.
//!
//! **How an agent tells them apart**, with no timing involved: `Value`'s object
//! is a `BTreeMap`, so a tree round-trip emits keys sorted, while `serde`'s
//! derived `Serialize` emits them in declaration order. The frame's fields are
//! declared `w, h, rows` and each row `y, dim, text` — neither in sorted order.
//! So the answer's own key order says which path produced it, at both depths.
//! The two are the same JSON *value* — key order carries no meaning in JSON, so
//! no consumer can be broken by the choice — which is exactly what makes it
//! safe to use as the witness.
//!
//! Run it: `cargo run -p hello-encoded-answer`. The window reports the frame's
//! size and the byte count of its encoding; the interesting half is over RPC.

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    RawJson, ReadRefusal, SchemaField, query_proxy_external_impl, read_only_or_unknown,
};
use pinion_core::scene::{ContainerNode, ExternalNode, Rect, Scene, TextNode};
use pinion_core::style::{BoxStyle, Color, FlexDirection, LayoutStyle, TextStyle};
use pinion_core::widgets::button::{ButtonEvent, ButtonState};
use pinion_core::{Frame as PaintFrame, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(EncodedAnswerRenderer, EncodedAnswerRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 360;
const SURFACE_TAG: &str = "frame_source";

/// Frame geometry. 80×24 is not decoration — it is the size the reporting
/// consumer polls, and small payloads hide the cost this round is about.
const COLS: usize = 80;
const ROWS: usize = 24;

/// One row of the served frame.
///
/// Declared `y, dim, text`; sorted order is `dim, text, y`. The gap is the
/// per-row half of the witness — an answer that kept declaration order at the
/// top level but sorted inside it would mean something was rebuilt part way
/// down, and this is what would catch it.
#[derive(Debug, serde::Serialize)]
struct Row {
    y: u16,
    dim: bool,
    text: String,
}

/// The whole frame. Declared `w, h, rows`; sorted order is `h, rows, w`.
#[derive(Debug, serde::Serialize)]
struct TerminalFrame {
    w: u16,
    h: u16,
    rows: Vec<Row>,
}

impl TerminalFrame {
    /// Build the frame from nothing but its geometry — deterministic, so every
    /// read of every instance answers identically and the demo can compare two
    /// answers taken at different moments without a settling step.
    fn generate() -> Self {
        let rows = (0..ROWS)
            .map(|y| {
                // A row of printable filler that varies per line, so the
                // payload is real text rather than one repeated byte a
                // compressor-shaped reading could dismiss.
                let text: String = (0..COLS)
                    .map(|x| {
                        let n = (y * COLS + x) % 62;
                        char::from(match n {
                            0..=9 => b'0' + u8::try_from(n).unwrap_or(0),
                            10..=35 => b'a' + u8::try_from(n - 10).unwrap_or(0),
                            _ => b'A' + u8::try_from(n - 36).unwrap_or(0),
                        })
                    })
                    .collect();
                Row {
                    y: u16::try_from(y).unwrap_or(u16::MAX),
                    dim: y % 2 == 1,
                    text,
                }
            })
            .collect();
        Self {
            w: u16::try_from(COLS).unwrap_or(u16::MAX),
            h: u16::try_from(ROWS).unwrap_or(u16::MAX),
            rows,
        }
    }
}

/// The §5.15 surface. Holds no cached encoding on purpose: the point is that
/// encoding *once, on demand* is cheaper than building a tree the envelope
/// then walks, not that a producer must keep a second copy of its own state.
#[derive(Debug, Default)]
struct FrameSource;

// The §5.15 `External` skeleton for a query-only node — Gui+Rpc, framework
// repaint, UI-thread sync, introspection opted in — is substrate, not something
// a binding re-types.
query_proxy_external_impl!(FrameSource);

impl ExternalIntrospect for FrameSource {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("frame", "json"),
                    SchemaField::new("frame_via_dom", "json"),
                    SchemaField::new("rows", "int"),
                    SchemaField::new("bytes", "int"),
                    // R1637 — the verb this example exists to demonstrate.
                    SchemaField::action("encode", "json"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        match path {
            // The production answer: one encoding pass, no tree.
            "frame" => Ok(IntrospectValue::raw(&TerminalFrame::generate())),
            // The control this example exists to be compared against.
            "frame_via_dom" => Ok(IntrospectValue::json(&TerminalFrame::generate())),
            "rows" => Ok(IntrospectValue::Int(i64::try_from(ROWS).unwrap_or(0))),
            // What the answer costs to carry, so a client can check that the
            // number it received is the number the producer meant to send
            // rather than trusting a length it computed on its own side.
            "bytes" => Ok(IntrospectValue::Int(
                encoded_len(&TerminalFrame::generate()).unwrap_or(0),
            )),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    /// Read-only: a served frame is a report of state, not a knob on it. The
    /// error still distinguishes "you cannot write this" from "this does not
    /// exist" — an agent told `ReadOnly` for a path that is not there learns
    /// something false about the surface (§2 #7).
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(read_only_or_unknown(&self.schema(), path))
    }

    fn invoke(
        &mut self,
        path: &str,
        _args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // `scene/invoke` is the other handler whose answer is the whole
            // result, so an action that computes a large payload gets the same
            // treatment a read does.
            "encode" => Ok(IntrospectValue::raw(&TerminalFrame::generate())),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Byte length of the frame's encoding, measured on the encoding itself
/// rather than estimated from the geometry.
fn encoded_len(frame: &TerminalFrame) -> Option<i64> {
    RawJson::encode(frame)
        .ok()
        .and_then(|raw| i64::try_from(raw.get().len()).ok())
}

fn line(text: &str, size: u32, fg: Color) -> Scene {
    let mut style = TextStyle::new();
    style.fg_color = fg;
    style.font_size_px = size;
    Scene::Text(TextNode::styled(text, Rect::default(), style))
}

/// view-fn (§6.3). Reports what the surface serves — the size of the answer
/// and the cost of carrying it — so the binding is readable without a client
/// attached, and states which encoding the production slot uses.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: ButtonState, _frame: &PaintFrame) -> Scene {
    let frame = TerminalFrame::generate();
    let bytes = encoded_len(&frame).unwrap_or(0);

    Scene::Container(
        ContainerNode::new(vec![
            line("pre-encoded answer", 22, Color::rgb(0x14, 0x14, 0x14)),
            line(
                &format!("serving a {COLS}x{ROWS} frame — {bytes} bytes of JSON"),
                15,
                Color::rgb(0x33, 0x33, 0x33),
            ),
            line(
                "frame          encoded once, spliced into the response",
                14,
                Color::rgb(0x1B, 0x5E, 0x20),
            ),
            line(
                "frame_via_dom  the same document through a Value tree",
                14,
                Color::rgb(0x88, 0x50, 0x00),
            ),
            line(
                &format!("first row: {}", &frame.rows[0].text[..32]),
                13,
                Color::rgb(0x55, 0x55, 0x55),
            ),
            Scene::External(
                ExternalNode::new(Box::new(FrameSource)).with_tag(SURFACE_TAG.to_owned()),
            ),
        ])
        .with_style(BoxStyle::filled(Color::rgb(0xFA, 0xFA, 0xFA)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_padding(Rect::new(24, 24, 24, 24))
                .with_gap(10),
        ),
    )
}

struct EncodedAnswer;

impl WidgetCore for EncodedAnswer {
    type State = ButtonState;
    type Event = ButtonEvent;
    fn create_external() -> Box<dyn External> {
        Box::new(FrameSource)
    }
    fn tag() -> &'static str {
        SURFACE_TAG
    }
    fn read_state(_scene: &Scene) -> Self::State {
        ButtonState::Idle
    }
    fn view(state: Self::State, frame: &PaintFrame) -> Scene {
        view(state, frame)
    }
    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }
    fn title() -> &'static str {
        "pinion pre-encoded answer demo"
    }
}

impl WidgetA11y for EncodedAnswer {}

impl WidgetView for EncodedAnswer {
    type Renderer = EncodedAnswerRenderer;
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
    fn windows() -> Vec<WindowSpec> {
        vec![WindowSpec::new(
            "main",
            "pinion pre-encoded answer demo",
            SizeStrategy::Fixed {
                width: WIN_W,
                height: WIN_H,
            },
        )]
    }
}

fn main() {
    pinion_shell::run::<EncodedAnswer>();
}

#[cfg(test)]
mod tests {
    use super::{COLS, ROWS, TerminalFrame, encoded_len};
    use pinion_core::external::{ExternalIntrospect, IntrospectValue, RawJson};

    #[test]
    fn r1480_the_served_frame_is_deterministic() {
        // Every assertion the demo makes compares two answers taken at
        // different moments; if generation drifted, those comparisons would
        // fail for a reason that has nothing to do with encoding.
        let a = RawJson::encode(&TerminalFrame::generate()).expect("encode");
        let b = RawJson::encode(&TerminalFrame::generate()).expect("encode");
        assert_eq!(a, b);
    }

    #[test]
    fn r1480_the_raw_slot_answers_in_declaration_order() {
        let source = super::FrameSource;
        let answer = source.query("frame").expect("the frame slot answers");
        let text = answer.as_raw().expect("the production slot is raw").get();
        assert!(
            text.starts_with(r#"{"w":80,"h":24,"rows":[{"y":0,"dim":false,"text":"#),
            "declaration order at both depths, got: {}",
            &text[..text.len().min(80)],
        );
    }

    #[test]
    fn r1480_the_control_slot_answers_through_a_tree() {
        let source = super::FrameSource;
        let answer = source.query("frame_via_dom").expect("the control answers");
        assert!(answer.as_raw().is_none(), "the control must be a DOM");
        let text = serde_json::to_string(&match answer {
            IntrospectValue::Json(v) => v,
            other => panic!("expected a Json answer, got {other:?}"),
        })
        .expect("serialize");
        assert!(
            text.starts_with(r#"{"h":24,"rows":[{"dim":false,"text":"#),
            "sorted at both depths, got: {}",
            &text[..text.len().min(80)],
        );
    }

    #[test]
    fn r1480_both_slots_carry_the_same_value() {
        let source = super::FrameSource;
        let raw = source
            .query("frame")
            .expect("raw")
            .as_raw()
            .expect("raw")
            .to_value()
            .expect("valid JSON");
        let dom = match source.query("frame_via_dom").expect("dom") {
            IntrospectValue::Json(v) => v,
            other => panic!("expected a Json answer, got {other:?}"),
        };
        assert_eq!(raw, dom, "one document, two encodings");
    }

    #[test]
    fn r1480_the_reported_byte_count_is_the_encoding_it_describes() {
        let frame = TerminalFrame::generate();
        let measured = encoded_len(&frame).expect("length fits");
        let actual = RawJson::encode(&frame).expect("encode").get().len();
        assert_eq!(
            usize::try_from(measured).expect("non-negative"),
            actual,
            "the surface must report the answer it actually sends",
        );
        // The payload has to be large enough for the round to be about
        // anything: 80x24 of text plus per-row envelopes.
        assert!(actual > COLS * ROWS, "got {actual} bytes");
    }
}
