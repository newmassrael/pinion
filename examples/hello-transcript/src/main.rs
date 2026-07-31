//! `hello-transcript` — R1445 §5.45 §5.27: **reveal what was just appended**,
//! when the appended extent is *layout-measured*.
//!
//! ## The gap this closes
//!
//! `hello-streaming-log` / `hello-paged-stream` append rows of a uniform
//! pitch, so they can name the post-append bound themselves (`count ×
//! row_pitch`) and hand it to
//! [`follow_tail`](pinion_core::widgets::virtual_list::follow_tail). A
//! transcript of **wrapped prose** cannot: how tall an entry is depends on
//! where parley breaks its lines, which nobody knows until taffy has run. The
//! bound exists — one pass later.
//!
//! Before R1445 that consumer had exactly one move: inflate the bound to
//! something the clamp cannot bite (`set_max(0, i32::MAX)`), pin past it, and
//! wait for the layout pass to drag the offset back down. It works, and it
//! publishes a bound that is **false for a frame** to every other reader of the
//! same [`ScrollState`] — including the view's own "is there more below?"
//! affordance.
//!
//! [`ScrollState::follow_measured_tail`] removes the need to name a bound at
//! all. The binding states the *intent* ("content grew; take me to the tail"),
//! and the layout pass — which is already writing the true bound every frame —
//! pins against the extent it measured. Every bound this app ever publishes is
//! a measured one.
//!
//! ## Two policies, one primitive
//!
//! The arming call **is** `follow_tail`'s `was_following` parameter, so the
//! policy stays with the binding. This app shows both halves of that fork:
//!
//! | control | policy | why |
//! |---|---|---|
//! | **Reply** | always reveal | the entry is the answer to what the reader just pressed; leaving it off-screen reads as "nothing happened" |
//! | **Notice** | reveal only if [`at_bottom`] held *before* the append | ambient traffic must not yank a reader who scrolled up (`tail -f`) |
//!
//! Neither reducer computes an extent. Both are two lines.
//!
//! ## AI-first witness (§2 #7)
//!
//! `scene/snapshot` carries every entry's tag + wrapped text + measured rect,
//! and the `Scroll` node's live `offset_y`; `scene/scroll` reports the bound
//! and `following_measured_tail` (a standing arming no pass has consumed yet).
//! The tail claim is checkable *without pixels*: the bound recomputed from the
//! published entry geometry must equal the published bound — which is exactly
//! the check an inflated `i32::MAX` bound fails. See
//! `tools/demos/r1445_transcript.py`.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::intent::Intent;
use pinion_core::reactive::{Owner, Signal};
use pinion_core::scene::{ContainerNode, Rect, ScrollNode, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::widgets::scroll::{ScrollState, use_scroll_state};
use pinion_core::widgets::virtual_list::at_bottom;
use pinion_core::{Command, Frame, Scene, WidgetCore, WidgetStateName};
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, button_scene, read_button_state,
};
use std::cell::RefCell;
use std::rc::Rc;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloTranscriptRenderer, HelloTranscriptRendererError);

const WIN_W: u32 = 480;
const WIN_H: u32 = 620;
const THEME_TAG: &str = "app";

/// Scroll viewport (logical px). Fixed, so the demo's expectations are
/// geometry-derived rather than window-manager-derived.
const VIEWPORT_W: u32 = 420;
const VIEWPORT_H: u32 = 300;
/// Padding inside the scrolled content, on all four sides.
const PAD: u32 = 12;
/// Gap between entries.
const GAP: u32 = 10;
/// Wrap width for one entry — the viewport minus the content padding. THIS is
/// the only geometry the binding states, and it says nothing about height.
const ENTRY_W: u32 = VIEWPORT_W - 2 * PAD;

/// Cache key + paint tag for the transcript's `ScrollState`.
const SCROLL_TAG: &str = "transcript_scroll";
/// Per-entry tag prefix (`entry#<i>`), and the a11y `list` container tag.
const ENTRY_TAG: &str = "entry";
/// `role=status` line: entry count + the derived follow state.
const STATUS_TAG: &str = "transcript_status";
/// The Reply button (primary External) — always reveals its entry.
const REPLY_TAG: &str = "transcript_reply";
/// The Notice button (extra External) — reveals only when already at the tail.
const NOTICE_TAG: &str = "transcript_notice";
/// `Owner::cache` key for the shared append-only transcript.
const SOURCE_KEY: &str = "transcript.source";

/// Fully-prefixed wire tags for the two controls' `"click"` intents (§5.20).
const REPLY_INTENT: &str = pinion_core::intent_tag!("transcript_reply", "click");
const NOTICE_INTENT: &str = pinion_core::intent_tag!("transcript_notice", "click");

/// Reply bodies, rotated per reply — deliberately different lengths, so each
/// entry wraps to a different number of lines and the total extent is
/// genuinely unknowable without laying it out.
const REPLIES: [&str; 4] = [
    "The pass runs after every bound writer, so the offset it pins is the one \
     the frame ended with.",
    "A bound is measured, never asserted. The binding says what it wants; the \
     layout pass says how far that is. Nothing in between has to guess, and no \
     reader of this state ever sees a number that was true for only one frame.",
    "Reply and notice differ in policy, not in machinery.",
    "Wrapped prose has no pitch to multiply. Its height is whatever the line \
     breaker decided, which is why the arming carries no number at all — the \
     one thing the caller genuinely cannot supply.",
];

/// Notice bodies — ambient traffic, shorter, and never the answer to anything
/// the reader pressed.
const NOTICES: [&str; 3] = [
    "A background task finished.",
    "Two peers reconnected; the queue drained without a retry.",
    "Nothing needed attention for the last interval.",
];

/// The backlog present at boot. Built through the *same* append path as every
/// later entry, so there is one text rule in this binding rather than a seed
/// dialect plus a runtime dialect — and a demo predicts entry text by counting
/// kinds, never by special-casing the seed.
const SEED: [Kind; 7] = [
    Kind::Notice,
    Kind::Reply,
    Kind::Reply,
    Kind::Notice,
    Kind::Reply,
    Kind::Reply,
    Kind::Notice,
];

// ─── shared append-only source ─────────────────────────────────────────────

/// What produced an entry — the axis the two follow policies split on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    /// The answer to a control the reader pressed.
    Reply,
    /// Ambient traffic nobody asked for.
    Notice,
}

/// The transcript's append-only entries plus the reactive `count` the view
/// subscribes to — the canonical shared-reactive-holder shape
/// (`hello-streaming-log`'s `LogSource` with prose instead of rows).
struct Transcript {
    entries: RefCell<Vec<(Kind, String)>>,
    count: Signal<usize>,
}

impl Transcript {
    /// Boots with a backlog already taller than the viewport, so "the reader
    /// scrolled back" is a reachable state from the first frame (a transcript
    /// that fits its viewport can never tell the two follow policies apart).
    fn new() -> Self {
        let this = Self {
            entries: RefCell::new(Vec::new()),
            count: Signal::new(0),
        };
        for kind in SEED {
            this.append(kind);
        }
        this
    }

    /// Append one entry of `kind` and publish the new total (the one
    /// observable change the view subscribes to). The body rotates over the
    /// per-kind pool by that kind's own running index, so a demo can predict
    /// every entry's text without predicting its height.
    fn append(&self, kind: Kind) {
        let mut entries = self.entries.borrow_mut();
        let nth = entries.iter().filter(|(k, _)| *k == kind).count();
        let body = match kind {
            Kind::Reply => REPLIES[nth % REPLIES.len()],
            Kind::Notice => NOTICES[nth % NOTICES.len()],
        };
        entries.push((kind, entry_text(kind, nth, body)));
        let total = entries.len();
        drop(entries);
        self.count.set(total);
    }
}

/// One entry's rendered text. Kept as a free fn so the demo can mirror it
/// exactly (the data witness for "the entry that appended is the one on
/// screen").
fn entry_text(kind: Kind, nth: usize, body: &str) -> String {
    match kind {
        Kind::Reply => format!("Reply {nth}: {body}"),
        Kind::Notice => format!("Notice {nth}: {body}"),
    }
}

fn use_transcript() -> Rc<Transcript> {
    Owner::current()
        .expect("use_transcript() requires an active Owner scope")
        .cache(SOURCE_KEY, Transcript::new)
}

// ─── the two reducers (this is the whole point) ────────────────────────────

/// A reply is the answer to what the reader just pressed, so it is revealed
/// wherever they were reading. No bound is named: the arming defers the pin to
/// the pass that measures the newly-wrapped entry.
fn apply_reply(src: &Transcript, scroll: &ScrollState) {
    src.append(Kind::Reply);
    scroll.follow_measured_tail();
}

/// Ambient traffic respects a reader who scrolled up — the `tail -f`
/// convention. The decision is read *before* the append (the bound is about to
/// grow), exactly as `follow_tail`'s `was_following` argument is.
fn apply_notice(src: &Transcript, scroll: &ScrollState) {
    let was_following = at_bottom(scroll.offset_y(), scroll.max().1);
    src.append(Kind::Notice);
    if was_following {
        scroll.follow_measured_tail();
    }
}

// ─── view ──────────────────────────────────────────────────────────────────

/// The `role=status` text — SSOT for the entry count + the derived follow
/// state (the same `at_bottom` predicate `apply_notice` branches on).
fn status_line(count: usize, following: bool) -> String {
    let mode = if following {
        "at the tail"
    } else {
        "scrolled back"
    };
    format!("{count} entries \u{00b7} {mode}")
}

/// One entry: a tagged, wrapped text leaf. Width is declared, height is
/// whatever the line breaker produces — the property that makes this
/// transcript's extent layout-measured.
fn build_entry(index: usize, kind: Kind, text: &str, theme: &Theme) -> Scene {
    let fg = match kind {
        Kind::Reply => theme.resolve(ColorRole::OnSurface),
        Kind::Notice => theme.resolve(ColorRole::OnSurfaceMuted),
    };
    Scene::Text(
        TextNode::styled(
            text.to_owned(),
            Rect::default(),
            TextStyle::new().with_size_px(14).with_fg(fg),
        )
        .with_tag(format!("{ENTRY_TAG}#{index}"))
        .with_layout(LayoutStyle::new().with_size(Size::width_px(ENTRY_W))),
    )
}

fn control(tag: &'static str, label: &str, state: ButtonState, theme: &Theme) -> Scene {
    button_scene(
        label,
        state,
        tag, // hover-spring key: the tag is already unique per control
        &ButtonColors::accent(theme),
        &ButtonStyle::m3_default(tag)
            .with_size(Size::px(190, 38))
            .with_corner_radius(19)
            .with_label_font_size_px(15),
    )
}

/// Both controls' interaction postures, read back from the two Externals.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Controls {
    reply: ButtonState,
    notice: ButtonState,
}

/// view-fn (§6.3): pure sync `(Controls) -> Scene`. Subscribes to the entry
/// count (re-render on append) and the scroll offset / bound (re-render when
/// the reader moves, or when the pin lands).
// `_frame` is `&Frame` because that is `WidgetCore::view`'s signature; the
// helper mirrors it so the trait impl is a one-line forward (same shape as
// `hello-streaming-log`).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: Controls, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let scroll = use_scroll_state(SCROLL_TAG);
    let src = use_transcript();

    let count = src.count.get();
    let following = at_bottom(scroll.offset_y(), scroll.max().1);

    let title = Scene::Text(TextNode::styled(
        "Transcript (wrapped prose, layout-measured tail)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(15)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let controls = Scene::Container(
        ContainerNode::new(vec![
            control(REPLY_TAG, "Reply", state.reply, &theme),
            control(NOTICE_TAG, "Notice", state.notice, &theme),
        ])
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center)
                .with_gap(12),
        ),
    );

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            status_line(count, following),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(STATUS_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

    let entries = src.entries.borrow();
    let content = Scene::Container(
        ContainerNode::new(
            entries
                .iter()
                .enumerate()
                .map(|(i, (kind, text))| build_entry(i, *kind, text, &theme))
                .collect(),
        )
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(GAP)
                .with_padding(Rect::new(PAD, PAD, PAD, PAD)),
        ),
    );
    let transcript = Scene::Scroll(ScrollNode::from_state(
        Rc::clone(&scroll),
        Rect::new(0, 0, VIEWPORT_W, VIEWPORT_H),
        content,
    ));

    Scene::Container(
        ContainerNode::new(vec![title, controls, status, transcript])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(14),
            ),
    )
}

struct TranscriptView;

impl WidgetCore for TranscriptView {
    type State = Controls;
    /// Every state change arrives as a button `"click"` intent (pointer) or
    /// through `apply_key` (keyboard); the shell's enum-typed keybinding
    /// channel is unused, so `()` satisfies the trait's `Copy` bound without
    /// an inhabited-but-dead event variant (mirror of `hello-card`).
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Touch the shared slots once at boot so the view and both reducers
        // resolve the same `Rc`s before the first paint.
        let _src = use_transcript();
        let _scroll = use_scroll_state(SCROLL_TAG);
        vec![ExtraExternal::new(
            NOTICE_TAG,
            Box::new(ButtonExternal::new()),
        )]
    }

    fn tag() -> &'static str {
        REPLY_TAG
    }

    fn read_state(scene: &Scene) -> Controls {
        Controls {
            reply: read_button_state(scene, REPLY_TAG),
            notice: read_button_state(scene, NOTICE_TAG),
        }
    }

    fn view(state: Controls, frame: &Frame) -> Scene {
        view(state, frame)
    }

    /// R1445 — the two append policies. Both run in an Owner scope, so they
    /// resolve the same `use_transcript` / `use_scroll_state` slots the view
    /// does; neither computes a scroll bound.
    fn update(_state: Controls, intent: &Intent) -> Vec<Command> {
        let tag = intent.tag_str();
        if tag == REPLY_INTENT {
            apply_reply(&use_transcript(), &use_scroll_state(SCROLL_TAG));
        } else if tag == NOTICE_INTENT {
            apply_notice(&use_transcript(), &use_scroll_state(SCROLL_TAG));
        }
        Vec::new()
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// ARIA button activation: Space / Enter on whichever control holds focus,
    /// in parity with a pointer click.
    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        use pinion_core::widgets::aria::apply_aria_activate;
        apply_aria_activate(scene, focused, key, REPLY_TAG)
            || apply_aria_activate(scene, focused, key, NOTICE_TAG)
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn title() -> &'static str {
        "pinion hello-transcript (R1445 §5.45 §5.27)"
    }

    fn fmt_state_log(state: &Controls) -> String {
        format!(
            "reply={} notice={}",
            state.reply.as_name(),
            state.notice.as_name()
        )
    }
}

impl WidgetA11y for TranscriptView {
    /// The two controls plus the transcript as a `list` of `listitem` entries
    /// (every entry is rendered — this view is not virtualized), and a
    /// `role=status` line carrying the count + follow state so an AT (and an
    /// agent) is told the tail moved, not just that a button was pressed.
    fn access_node(state: &Controls, focused: Option<&str>) -> Vec<AccessNode> {
        let scroll = use_scroll_state(SCROLL_TAG);
        let src = use_transcript();
        let entries = src.entries.borrow();
        let set_size = u32::try_from(entries.len()).unwrap_or(u32::MAX);

        let mut list = AccessNode::new(ENTRY_TAG, AriaRole::List).with_name("Transcript");
        for index in 0..entries.len() {
            list = list.with_child(format!("{ENTRY_TAG}#{index}"));
        }

        let mut nodes = vec![
            AccessNode::new(REPLY_TAG, AriaRole::Button)
                .with_name("Reply")
                .with_state(button_a11y_state(state.reply, focused == Some(REPLY_TAG))),
            AccessNode::new(NOTICE_TAG, AriaRole::Button)
                .with_name("Notice")
                .with_state(button_a11y_state(state.notice, focused == Some(NOTICE_TAG))),
            AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_line(
                entries.len(),
                at_bottom(scroll.offset_y(), scroll.max().1),
            )),
            list,
        ];
        for (index, (_, text)) in entries.iter().enumerate() {
            nodes.push(
                AccessNode::new(format!("{ENTRY_TAG}#{index}"), AriaRole::ListItem)
                    .with_name(text.clone())
                    .with_position_in_set(u32::try_from(index + 1).unwrap_or(u32::MAX))
                    .with_size_of_set(set_size),
            );
        }
        nodes
    }
}

impl WidgetView for TranscriptView {
    type Renderer = HelloTranscriptRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<TranscriptView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Boot the shared cache slots. Unlike `hello-streaming-log`'s `boot`,
    /// this deliberately does NOT seed a bound or a measured viewport: the
    /// binding under test is the one that never names either.
    fn boot() -> (Rc<Transcript>, Rc<ScrollState>) {
        (use_transcript(), use_scroll_state(SCROLL_TAG))
    }

    fn seeded(kind: Kind) -> usize {
        SEED.iter().filter(|k| **k == kind).count()
    }

    #[test]
    fn r1445_reply_arms_the_pin_without_naming_a_bound() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            let before = scroll.max();
            apply_reply(&src, &scroll);
            assert_eq!(src.count.get(), SEED.len() + 1, "the reply appended");
            assert!(
                scroll.is_following_measured_tail(),
                "the reveal is armed for the next layout pass",
            );
            assert_eq!(
                scroll.max(),
                before,
                "the binding published no bound of its own — the pass owns it",
            );
        });
    }

    #[test]
    fn r1445_notice_respects_a_reader_who_scrolled_back() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            // Stand in for a laid-out frame: a bound exists and the reader
            // has scrolled up from the tail.
            scroll.set_max(0, 400);
            scroll.scroll_to(0, 120);
            apply_notice(&src, &scroll);
            assert_eq!(src.count.get(), SEED.len() + 1, "the notice still appended",);
            assert!(
                !scroll.is_following_measured_tail(),
                "ambient traffic must not yank a reader who scrolled back",
            );
        });
    }

    #[test]
    fn r1445_notice_follows_a_reader_parked_at_the_tail() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            scroll.set_max(0, 400);
            scroll.scroll_to(0, 400);
            apply_notice(&src, &scroll);
            assert!(
                scroll.is_following_measured_tail(),
                "at the tail, ambient traffic keeps following it",
            );
        });
    }

    #[test]
    fn r1445_reply_reveals_even_from_the_top() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            scroll.set_max(0, 400);
            scroll.scroll_to(0, 0);
            apply_reply(&src, &scroll);
            assert!(
                scroll.is_following_measured_tail(),
                "an answer is revealed wherever the reader was — the fork \
                 `apply_notice` takes the other side of",
            );
        });
    }

    #[test]
    fn r1445_entry_bodies_rotate_per_kind() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            // Interleave, to pin that each kind rotates on ITS OWN running
            // index (the demo mirrors this to predict entry text).
            apply_reply(&src, &scroll);
            apply_notice(&src, &scroll);
            apply_reply(&src, &scroll);
            let (r, n) = (seeded(Kind::Reply), seeded(Kind::Notice));
            let entries = src.entries.borrow();
            assert_eq!(entries.len(), SEED.len() + 3);
            assert_eq!(
                entries[SEED.len()].1,
                entry_text(Kind::Reply, r, REPLIES[r % REPLIES.len()]),
            );
            assert_eq!(
                entries[SEED.len() + 1].1,
                entry_text(Kind::Notice, n, NOTICES[n % NOTICES.len()]),
            );
            assert_eq!(
                entries[SEED.len() + 2].1,
                entry_text(Kind::Reply, r + 1, REPLIES[(r + 1) % REPLIES.len()]),
            );
        });
    }

    #[test]
    fn r1445_view_renders_one_tagged_entry_per_append() {
        let owner = Owner::new();
        owner.run(|| {
            let (src, scroll) = boot();
            apply_reply(&src, &scroll);
            let scene = view(Controls::default(), &Frame::default());
            let n = SEED.len() + 1;
            for i in 0..n {
                let tag = format!("{ENTRY_TAG}#{i}");
                assert!(find_tag(&scene, &tag), "entry {tag} is in the scene");
            }
            assert!(
                !find_tag(&scene, &format!("{ENTRY_TAG}#{n}")),
                "no entry beyond the count",
            );
        });
    }

    /// Depth-first "is this tag anywhere in the scene" (entries live inside
    /// the Scroll's content, which `rect_for_tag` reaches only post-layout).
    fn find_tag(scene: &Scene, tag: &str) -> bool {
        match scene {
            Scene::Text(t) => t.tag.as_deref() == Some(tag),
            Scene::Container(c) => {
                c.tag.as_deref() == Some(tag) || c.children.iter().any(|ch| find_tag(ch, tag))
            }
            Scene::Scroll(s) => s.tag.as_deref() == Some(tag) || find_tag(s.content.as_ref(), tag),
            _ => false,
        }
    }
}
