//! `hello-refused-invoke` — R1564 §5.15 §5.12 §2 #2 (PINION-PR82).
//!
//! A binding whose one action refuses for two reasons the caller must act on
//! differently — and says which.
//!
//! # The case this models
//!
//! It is not invented. PINION-PR82 measured a consumer's fifteen reachable CLI
//! failure paths and found **six** printing a list of causes joined by `or`,
//! not because the consumer was lazy but because the daemon's handler knew
//! which cause it was and the wire had no slot for the answer. Its own example:
//! `report_agent` refuses in exactly two places — "no detector is installed"
//! and "no pane with that id" — and those demand completely different operator
//! actions. They arrived fused, as the string `InvokeRejected`.
//!
//! `report` here is that method. It refuses when no detector is installed, and
//! when the named pane does not exist. Before R1564 both produced the identical
//! JSON-RPC frame; the demo asserts they no longer do, and asserts it over a
//! real socket rather than against the trait.
//!
//! # What the round changed, and why the two halves are one change
//!
//! `InvokeError::Rejected` carries a
//! [`RefusalReason`](pinion_core::external::RefusalReason) the producer writes, and
//! `pinion-rpc` forwards it to `error.data` **verbatim**. That alone would make
//! the wire *less* machine-readable, not more: `data` was previously always a
//! word this framework authored (`"UnknownInvokePath"`), so a client could
//! match one to classify, and free application prose in the same slot takes
//! that away. So the reason arrives under
//! [`ACTION_REFUSED`](pinion_rpc::ACTION_REFUSED) — a code distinct from
//! `-32602 Invalid params`, which is the wrong category anyway for a call whose
//! parameters were fine. A consumer branches on the code and *prints* the
//! sentence; it never matches the sentence.
//!
//! # Past the toolkit
//!
//! The toolkit has no channel at all here: `invokeMethod` answers `bool`,
//! `trigger()` answers `void`. A toolkit method that declines can log to
//! stderr in the process that declined and that is the end of it — nothing
//! reaches an out-of-process caller. So none of this is parity; the shape is
//! chosen ([[the toolkit-is-the-floor-not-the-target]]).
//!
//! Two further things the surface does that the toolkit's has no place to put:
//!
//!   * `install_detector` / `evict_pane` make both refusal *causes* reachable
//!     over the same wire, so a client can drive the surface into either state
//!     and read the sentence back — the refusal is a testable property of the
//!     API rather than something only reproducible by hand;
//!   * the vocabulary a refusal advertises is the vocabulary the surface
//!     actually accepts, because both are derived from one declaration (see
//!     [`WidgetEventName::drivable_names`](pinion_core::WidgetEventName::drivable_names)),
//!     so a refusal cannot advertise a name the surface would then decline.
//!
//! Run it: `cargo run -p hello-refused-invoke`. The window states the surface's
//! current preconditions; the RPC half is the interesting one
//! (`tools/demos/r1564_refusal_states_why.py`).

use pinion_a11y::WidgetA11y;
use pinion_core::external::{
    External, ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError,
    ReadRefusal, SchemaArg, SchemaField, query_proxy_external_impl, read_only_or_unknown,
};
use pinion_core::scene::{ContainerNode, Rect, Scene, TextNode};
use pinion_core::style::{BoxStyle, Color, FlexDirection, LayoutStyle, TextStyle};
use pinion_core::widgets::button::{ButtonEvent, ButtonState};
use pinion_core::{Frame as PaintFrame, WidgetCore};
use pinion_shell::{SizeStrategy, WidgetView, WindowSpec, vello_renderer_impl};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(RefusedInvokeRenderer, RefusedInvokeRendererError);

const WIN_W: u32 = 720;
const WIN_H: u32 = 300;

/// The binding's own tag — [`WidgetCore::tag`], so this is the node
/// `CoreShell` puts in the state scene.
const HOST_TAG: &str = "host";

/// The panes this host starts with. Small and fixed so the demo's assertions
/// can name a pane that exists and one that does not, without a fixture.
const BOOT_PANES: [u32; 3] = [1, 2, 3];

/// The largest report count this host will accept on a restore. Small and
/// fixed so the demo can name a value outside it without a fixture.
const MAX_REPORTS: i64 = 1_000;

// ---------------------------------------------------------------------------
// the surface
// ---------------------------------------------------------------------------

/// A host with panes and an optional agent detector.
///
/// The shape is PINION-PR82's `report_agent` reduced to its two preconditions.
/// Both are *states of this object*, not of the argument, which is precisely
/// why `TypeMismatch` cannot express either: the argument was a perfectly good
/// pane id in both cases.
#[derive(Debug)]
struct Host {
    /// Panes this host is currently tiling.
    panes: Vec<u32>,
    /// Whether an agent detector is installed. `report` needs one.
    detector: bool,
    /// How many reports have succeeded — queryable, so a refusal is checkable
    /// against the fact that nothing happened, not merely reported.
    reports: i64,
}

impl Default for Host {
    fn default() -> Self {
        Self {
            panes: BOOT_PANES.to_vec(),
            detector: false,
            reports: 0,
        }
    }
}

impl Host {
    /// R1564 — the two refusals `report` can produce, each naming the fact this
    /// object observed.
    ///
    /// Written as one function returning `Result<(), InvokeError>` rather than
    /// as two guards inside the arm, because the *point* is that they are two
    /// values of one type: the caller distinguishes them by reading them, and
    /// this is the one place that decides what each says.
    fn report_preconditions(&self, pane: u32) -> Result<(), InvokeError> {
        if !self.detector {
            return Err(InvokeError::rejected(
                "report: no agent detector is installed on this host; \
                 install one with install_detector",
            ));
        }
        if !self.panes.contains(&pane) {
            return Err(InvokeError::rejected(format!(
                "report: no pane {pane} on this host (it has {})",
                self.pane_list()
            )));
        }
        Ok(())
    }

    /// The live pane ids, for a refusal that names what IS there. A refusal
    /// that says only what is absent leaves the caller a second round trip
    /// away from acting on it.
    fn pane_list(&self) -> String {
        self.panes
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    }
}

query_proxy_external_impl!(Host);

impl ExternalIntrospect for Host {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("panes", "string"),
                    SchemaField::new("detector", "bool"),
                    SchemaField::new("reports", "int"),
                    SchemaField::action("report", "int"),
                    SchemaField::action("install_detector", "bool"),
                    SchemaField::action("evict_pane", "int"),
                    // Parametric, so the demo can ask about a pane BEFORE
                    // driving an action at it — the read half of the same fact
                    // the refusal states.
                    SchemaField::parametric(
                        "has_pane.<id>",
                        "bool",
                        const { &[SchemaArg::open("id", "int")] },
                    ),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Result<IntrospectValue, ReadRefusal> {
        if let Some(rest) = path.strip_prefix("has_pane.") {
            let id: u32 = rest.parse().map_err(|_| ReadRefusal::QueryTypeMismatch)?;
            return Ok(IntrospectValue::Bool(self.panes.contains(&id)));
        }
        match path {
            "panes" => Ok(IntrospectValue::Text(self.pane_list())),
            "detector" => Ok(IntrospectValue::Bool(self.detector)),
            "reports" => Ok(IntrospectValue::Int(self.reports)),
            _ => Err(ReadRefusal::UnknownPath),
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // R1565 — one writable slot, and it exists to carry the write channel's
        // half of this round onto the wire: a value outside a range refuses with
        // the RANGE, under a code of its own. `reports` is a counter, so "how
        // many reports has this host recorded" is a legitimate restore slot
        // (a session resumes mid-count) with an obvious bound.
        if path == "reports" {
            let IntrospectValue::Int(n) = value else {
                return Err(InterveneError::TypeMismatch);
            };
            if !(0..=MAX_REPORTS).contains(&n) {
                return Err(InterveneError::out_of_range(format!(
                    "a report count runs 0..={MAX_REPORTS}, and {n} is outside it"
                )));
            }
            self.reports = n;
            return Ok(());
        }
        // Every other slot is derived from what the ACTIONS did; a direct write
        // would let the demo fake the state a refusal reports on.
        Err(read_only_or_unknown(&self.schema(), path))
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match (path, args) {
            ("report", IntrospectValue::Int(id)) => {
                let pane = u32::try_from(id).map_err(|_| {
                    // R1564 — a malformed ADDRESS is still a refusal (the type
                    // matched), and it says which of the three it is.
                    InvokeError::rejected(format!("report: {id} is not a pane id"))
                })?;
                self.report_preconditions(pane)?;
                self.reports += 1;
                Ok(IntrospectValue::Int(self.reports))
            }
            ("install_detector", _) => {
                if self.detector {
                    return Err(InvokeError::rejected(
                        "install_detector: a detector is already installed on this host",
                    ));
                }
                self.detector = true;
                Ok(IntrospectValue::Bool(true))
            }
            ("evict_pane", IntrospectValue::Int(id)) => {
                let pane = u32::try_from(id).map_err(|_| {
                    InvokeError::rejected(format!("evict_pane: {id} is not a pane id"))
                })?;
                let before = self.panes.len();
                self.panes.retain(|p| *p != pane);
                if self.panes.len() == before {
                    return Err(InvokeError::rejected(format!(
                        "evict_pane: no pane {pane} on this host (it has {})",
                        self.pane_list()
                    )));
                }
                Ok(IntrospectValue::Int(i64::from(pane)))
            }
            // Both int-taking actions share the arm: a non-`Int` argument is the
            // same finding for either, and it is the framework's, not the
            // surface's — so it carries no reason.
            ("report" | "evict_pane", _) => Err(InvokeError::TypeMismatch),
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

// ---------------------------------------------------------------------------
// paint
// ---------------------------------------------------------------------------

fn row(text: String) -> Scene {
    Scene::Text(TextNode::styled(text, Rect::default(), TextStyle::new()))
}

fn view(_state: ButtonState) -> Scene {
    // The window is deliberately plain: this binding exists for the wire, and
    // the paint states only what a reader would otherwise have to ask for.
    Scene::Container(
        ContainerNode::new(vec![
            row("hello-refused-invoke — a refusal states why".to_owned()),
            row("invoke report <pane>: refuses without a detector,".to_owned()),
            row("and refuses again for a pane this host does not have.".to_owned()),
            row("Both used to be the string \"InvokeRejected\".".to_owned()),
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

struct RefusedInvoke;

impl WidgetCore for RefusedInvoke {
    type State = ButtonState;
    type Event = ButtonEvent;
    fn create_external() -> Box<dyn External> {
        Box::new(Host::default())
    }
    fn tag() -> &'static str {
        HOST_TAG
    }
    fn read_state(_scene: &Scene) -> Self::State {
        ButtonState::Idle
    }
    fn view(state: Self::State, _frame: &PaintFrame) -> Scene {
        view(state)
    }
    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }
    fn title() -> &'static str {
        "pinion refused-invoke demo"
    }
}

impl WidgetA11y for RefusedInvoke {}

impl WidgetView for RefusedInvoke {
    type Renderer = RefusedInvokeRenderer;
    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
    fn windows() -> Vec<WindowSpec> {
        vec![WindowSpec::new(
            "main",
            "pinion refused-invoke demo",
            SizeStrategy::Fixed {
                width: WIN_W,
                height: WIN_H,
            },
        )]
    }
}

fn main() {
    pinion_shell::run::<RefusedInvoke>();
}

#[cfg(test)]
mod tests {
    use super::{HOST_TAG, Host};
    use pinion_core::external::{ExternalIntrospect, IntrospectValue};
    use pinion_core::scene::{ExternalNode, Scene};
    use pinion_core::test_fixtures::assert_refused_saying;
    use pinion_rpc::{ACTION_REFUSED, InvokeError as RpcInvokeError};

    fn state_scene() -> Scene {
        Scene::External(ExternalNode::new(Box::new(Host::default())).with_tag(HOST_TAG.to_owned()))
    }

    #[test]
    fn r1564_one_action_refuses_two_ways_and_says_which() {
        // THE claim, and PINION-PR82's own case: `report` refuses because no
        // detector is installed, and refuses because the pane is not there.
        // Pre-R1564 both were the value `InvokeError::Rejected` — equal to each
        // other, so this test could not have been written at all.
        let mut host = Host::default();
        let no_detector = host.invoke("report", IntrospectValue::Int(1));
        assert_refused_saying(&no_detector, "no agent detector is installed");

        host.invoke("install_detector", IntrospectValue::Null)
            .expect("a fresh host has no detector");
        let no_pane = host.invoke("report", IntrospectValue::Int(99));
        assert_refused_saying(&no_pane, "no pane 99 on this host");

        assert_ne!(
            no_detector.unwrap_err(),
            no_pane.unwrap_err(),
            "two refusals of one action are two VALUES, which is what lets a \
             consumer stop guessing between them",
        );
        assert_eq!(
            host.query("reports"),
            Ok(IntrospectValue::Int(0)),
            "neither refusal ran the action",
        );
    }

    #[test]
    fn r1564_a_refusal_names_what_is_there_not_only_what_is_missing() {
        // A sentence that says only "no pane 99" leaves the caller a round trip
        // from acting on it. The live set is the fact it needs.
        let mut host = Host::default();
        host.invoke("install_detector", IntrospectValue::Null)
            .expect("installs");
        let refusal = host.invoke("report", IntrospectValue::Int(99));
        assert_refused_saying(&refusal, "it has 1, 2, 3");

        // And it tracks the surface rather than being a fixed string: evicting
        // a pane changes what the next refusal reports.
        host.invoke("evict_pane", IntrospectValue::Int(2))
            .expect("pane 2 is there");
        assert_refused_saying(
            &host.invoke("report", IntrospectValue::Int(99)),
            "it has 1, 3",
        );
    }

    #[test]
    fn r1564_the_refusal_agrees_with_the_read() {
        // §2 #7 — the refusal is a statement about the scene, so the read
        // channel has to corroborate it. A refusal naming a pane `has_pane`
        // reports as present would be the class of false statement R1487
        // removed from this very method.
        let mut host = Host::default();
        host.invoke("install_detector", IntrospectValue::Null)
            .expect("installs");
        assert_eq!(host.query("has_pane.99"), Ok(IntrospectValue::Bool(false)));
        assert_refused_saying(
            &host.invoke("report", IntrospectValue::Int(99)),
            "no pane 99",
        );
        assert_eq!(host.query("has_pane.2"), Ok(IntrospectValue::Bool(true)));
        assert!(
            host.invoke("report", IntrospectValue::Int(2)).is_ok(),
            "a pane the read reports present is one the action accepts",
        );
    }

    #[test]
    fn r1564_the_typed_dispatcher_carries_the_reason_under_its_own_code() {
        // The transport half, through the same dispatcher the socket uses.
        // `UnknownInvokePath` stays a framework word; a refusal is the
        // producer's sentence, and the two are told apart by the VARIANT here
        // and by the CODE on the wire.
        let mut scene = state_scene();
        let refusal = pinion_rpc::invoke(&mut scene, "/external/report", IntrospectValue::Int(1))
            .expect_err("no detector on a fresh host");
        match &refusal {
            RpcInvokeError::InvokeRejected(reason) => assert!(
                reason.as_str().contains("no agent detector is installed"),
                "the producer's sentence reaches the transport verbatim, got {reason:?}",
            ),
            other => panic!("expected a stated refusal, got {other:?}"),
        }

        let unknown = pinion_rpc::invoke(&mut scene, "/external/nope", IntrospectValue::Null)
            .expect_err("no such action");
        assert_eq!(
            unknown,
            RpcInvokeError::UnknownInvokePath,
            "a path the SCHEMA does not declare is the framework's finding, not \
             the producer's, so it stays a framework word",
        );

        // The code split is the consumer-visible half of that distinction.
        assert_eq!(ACTION_REFUSED, -32005);
        assert_ne!(ACTION_REFUSED, -32602);
    }

    #[test]
    fn r1564_an_action_that_would_change_nothing_is_refused_by_name() {
        // `install_detector` twice: the second is not an error of the caller's
        // making and not a silent no-op either. Naming it is what lets a
        // client tell "already done" from "did it".
        let mut host = Host::default();
        assert_eq!(
            host.invoke("install_detector", IntrospectValue::Null),
            Ok(IntrospectValue::Bool(true))
        );
        assert_refused_saying(
            &host.invoke("install_detector", IntrospectValue::Null),
            "a detector is already installed",
        );
    }

    #[test]
    fn r1564_a_wrong_arg_type_is_still_not_a_refusal() {
        // The boundary the variant has always drawn, and it must survive a
        // round that made the neighbouring variant richer: a `Text` where an
        // `Int` belongs cannot succeed on retry with the same shape, so it is
        // `TypeMismatch` and carries no producer sentence.
        let mut host = Host::default();
        let err = host
            .invoke("report", IntrospectValue::Text("one".to_owned()))
            .expect_err("report takes an int");
        assert_eq!(err, pinion_core::external::InvokeError::TypeMismatch);
        assert!(
            err.reason().is_none(),
            "only a refusal carries a reason; a type error's meaning IS its variant",
        );
    }
}
