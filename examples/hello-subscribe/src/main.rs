//! `hello-subscribe` — R1552 §5.7 PINION-PR83: the reference consumer of the
//! **change stream** — one request the server answers many times.
//!
//! ## Why this exists (the sprag `events.subscribe` case)
//!
//! Before R1552 an [`RpcFrame`](pinion_rpc::RpcFrame) held exactly one
//! [`RpcReply`](pinion_rpc::RpcReply), a `FnOnce`. So a client that wanted to
//! follow the scene had to ask again after every answer: `scene/waitFor
//! {since: N}` parks, fires once, and the client re-issues with `N+1`. That is
//! correct — the revision cursor is half-open, so nothing is missed — and it
//! costs a socket round trip per change. PINION-PR83 reported the shape: "one
//! request, many responses" was not merely unimplemented, it was
//! **inexpressible**, because the transport had no way to write a frame nobody
//! had asked for.
//!
//! R1552 adds [`RpcEgress`](pinion_rpc::RpcEgress) — the connection's writer,
//! the mirror of `RpcIngress` — and this binding is what proves it end to end.
//!
//! ## What this demonstrates
//!
//! The view paints two facts and nothing else, both read live:
//!
//! * **The scene's own revision**, from the one
//!   [`SceneRevision`](pinion_core::SceneRevision) token the whole app shares
//!   ([`use_scene_revision`]). This is the number a subscriber is told about,
//!   so the painted digit and the `scene/changed` notification's `revision`
//!   are the *same* value read two ways — the demo asserts exactly that.
//! * **The live change streams**, from
//!   [`pinion_rpc::process_registry`] — how many are open, which connections
//!   own them, and how many notifications this process has written. §2 #7: an
//!   agent learns who is listening to this app, over the wire, with no pixels.
//!   Qt publishes no equivalent for `QLocalServer`, because nothing in Qt binds
//!   a server-initiated write to a *named* stream in the first place.
//!
//! It is display-only (PR-51 `primary_surface() -> None`, as
//! `hello-conn-lifecycle`): its input arrives out-of-band, over RPC.
//!
//! ## The two transports, deliberately
//!
//! The built-in **stdin** connection is one connection, and its egress is
//! stdout — so a client driving this binary over a pipe can subscribe and read
//! its own stream interleaved with its ordinary responses. That interleaving is
//! the point, and is why the notification is a JSON-RPC *notification* (a
//! `method`, no `id`) rather than a second response: it is the one form a
//! client keyed on its own pending ids can tell apart from its answer.
//!
//! The **socket** transport is mounted for what stdin cannot show: two
//! independent clients, each with its own stream, so closing one leaves the
//! other's intact — and so a client that simply *vanishes* has its stream
//! released by the `on_disconnect` hook, with no `scene/unsubscribe` ever sent.
//!
//! ## Verification
//!
//! `tools/demos/r1552_subscribe.py`.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextAlign, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{External, Frame, Scene, WidgetCore};
use pinion_rpc::{RpcIngress, SubscriptionsOutcome};
use pinion_rpc_transport::{TransportControl, UnixSocketTransport};
use pinion_shell::{
    ShellConfig, SizeStrategy, WidgetView, run_with_config, use_scene_revision, vello_renderer_impl,
};

// pinion-forge codegen output — defines `HelloSubscribeRenderer` +
// `HelloSubscribeRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloSubscribeRenderer, HelloSubscribeRendererError);

const WIN_W: u32 = 470;
const WIN_H: u32 = 360;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

const REVISION_TAG: &str = "sub_revision";
const STREAMS_TAG: &str = "sub_streams";
const PUBLISHED_TAG: &str = "sub_published";
const LIST_TAG: &str = "sub_list";
const HINT_TAG: &str = "sub_hint";

/// Env var naming the socket path to bind. The demo sets a unique path; a bare
/// `cargo run` falls back to a per-pid temp path (printed in the hint).
const SOCK_ENV: &str = "PINION_SUBSCRIBE_SOCK";

/// The socket transport's lifetime handle, parked for the process lifetime (the
/// app owns its endpoint; dropping it would unbind the socket), plus the
/// resolved path for the human-facing hint line.
static CONTROL: Mutex<Option<TransportControl>> = Mutex::new(None);
static SOCKET_PATH: Mutex<Option<String>> = Mutex::new(None);

// ─── scene-as-data rendering (pure; the view is a thin wrapper) ──────────────

/// The revision line — the single SSOT for the painted text and its
/// `role=status` accessible name.
///
/// This is the number a `scene/changed` notification carries. Painting it is
/// what makes "my subscription is telling me the truth" checkable without
/// trusting either side alone.
fn revision_line(revision: u64) -> String {
    format!("Scene revision: {revision}")
}

/// The live-stream count line — the SSOT for its painted text and a11y name.
fn streams_line(count: usize) -> String {
    match count {
        0 => "0 live change streams".to_owned(),
        1 => "1 live change stream".to_owned(),
        n => format!("{n} live change streams"),
    }
}

/// The published-total line — notifications this process has written, across
/// every stream including closed ones.
fn published_line(total: u64) -> String {
    match total {
        1 => "1 notification published".to_owned(),
        n => format!("{n} notifications published"),
    }
}

/// One stream's row text — the SSOT for the painted row and its `listitem`
/// accessible name. Both ids are opaque; only their identity matters.
fn stream_row_text(subscription: u64, conn: u64, delivered: u64) -> String {
    format!("stream #{subscription} on conn #{conn} — {delivered} delivered")
}

/// Stable per-row tag shared by the painted row and its `listitem`
/// `AccessNode`, so AT bounds attach to the paint.
fn row_tag(visible_index: usize) -> String {
    format!("sub_row_{visible_index}")
}

/// The human-facing hint (socket path); empty in tests, where nothing seeded
/// [`SOCKET_PATH`].
fn socket_hint() -> String {
    match SOCKET_PATH.lock().unwrap().as_ref() {
        Some(path) => format!("Socket: {path}"),
        None => String::new(),
    }
}

/// One tagged, horizontally-centred line of text — the three status rows and
/// the hint differ only in their [`TextStyle`].
fn tagged_centered_line(
    tag: impl Into<std::borrow::Cow<'static, str>>,
    text: String,
    style: TextStyle,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            text,
            Rect::default(),
            style,
        ))])
        .with_tag(tag)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    )
}

/// One stream row — a tagged container holding the single SSOT line.
fn stream_row_scene(
    visible_index: usize,
    view: &pinion_rpc::SubscriptionView,
    theme: &pinion_core::Theme,
) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            stream_row_text(view.subscription, view.conn, view.delivered_count),
            Rect::default(),
            TextStyle::new()
                .with_size_px(13)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(row_tag(visible_index))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(WIN_W - 96, 24)),
        ),
    )
}

/// The streams panel — the live rows, or a placeholder when none.
/// `LIST_TAG` stays present across both states.
fn stream_list_scene(streams: &SubscriptionsOutcome, theme: &pinion_core::Theme) -> Scene {
    let children: Vec<Scene> = if streams.subscriptions.is_empty() {
        vec![Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                "No streams \u{2014} send scene/subscribe to open one".to_owned(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(13)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
                    .with_align(TextAlign::Center),
            ))])
            .with_tag(row_tag(0))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(WIN_W - 96, 24)),
            ),
        )]
    } else {
        streams
            .subscriptions
            .iter()
            .enumerate()
            .map(|(i, s)| stream_row_scene(i, s, theme))
            .collect()
    };

    Scene::Container(
        ContainerNode::new(children)
            .with_tag(LIST_TAG)
            .with_style(
                BoxStyle::filled(theme.resolve(ColorRole::SurfaceContainerLow))
                    .with_corner_radius(8),
            )
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_gap(2)
                    .with_padding(Rect::new(10, 10, 10, 10))
                    .with_size(Size::px(WIN_W - 56, 150)),
            ),
    )
}

/// Pure panel builder: `(revision, live streams) -> Scene`, resolving the theme
/// from the active owner scope. Split out so tests drive it with explicit
/// inputs instead of standing up a transport.
fn subscribe_panel_scene(revision: u64, streams: &SubscriptionsOutcome) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(TextNode::styled(
        "Change streams (scene/subscribe)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let revision_row = tagged_centered_line(
        REVISION_TAG,
        revision_line(revision),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    );
    let streams_row = tagged_centered_line(
        STREAMS_TAG,
        streams_line(streams.subscriptions.len()),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    );
    let published_row = tagged_centered_line(
        PUBLISHED_TAG,
        published_line(streams.published_total),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    );
    let hint = tagged_centered_line(
        HINT_TAG,
        socket_hint(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
            .with_align(TextAlign::Center),
    );

    Scene::Container(
        ContainerNode::new(vec![
            title,
            revision_row,
            streams_row,
            published_row,
            stream_list_scene(streams, &theme),
            hint,
        ])
        .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_align_items(AlignItems::Center)
                .with_justify(JustifyContent::Center)
                .with_size(Size::px(WIN_W, WIN_H))
                .with_gap(10),
        ),
    )
}

/// view-fn (§6.3): pure sync `() -> Scene`, reading the two live sources — the
/// one scene revision token and the process's stream registry.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    subscribe_panel_scene(
        use_scene_revision().current(),
        &pinion_rpc::process_registry().views(),
    )
}

/// The binding. Hand-written (not `#[widget]`-derived) because it is
/// display-only: its sole input arrives out-of-band over RPC, never through a
/// pointer. PR-51's `primary_surface()` opt-out is exactly this shape.
struct SubscribeView;

impl WidgetCore for SubscribeView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: no statechart, no captured gesture — the
    /// RPC wire drives this scene, not an External.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-subscribe has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-subscribe has no primary surface — see primary_surface()")
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        Vec::new()
    }

    fn read_state(_scene: &Scene) -> Self::State {}

    fn view(state: Self::State, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: Self::Event) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-subscribe (R1552 §5.7 PINION-PR83)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (input arrives over the RPC wire)".to_owned()
    }
}

impl WidgetA11y for SubscribeView {
    /// A WAI-ARIA `list` over the live streams (one `listitem` each, named from
    /// the same SSOT the paint uses) plus three `role=status` live regions —
    /// the revision, the stream count and the published total all change
    /// out-of-band, which is what a live region is for.
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        let streams = pinion_rpc::process_registry().views();
        let revision = use_scene_revision().current();

        let mut list = AccessNode::new(LIST_TAG, AriaRole::List).with_name("Live change streams");
        let mut items: Vec<AccessNode> = Vec::new();
        for (i, s) in streams.subscriptions.iter().enumerate() {
            let tag = row_tag(i);
            list = list.with_child(tag.clone());
            items.push(
                AccessNode::new(tag, AriaRole::ListItem)
                    .with_name(stream_row_text(s.subscription, s.conn, s.delivered_count))
                    .with_position_in_set(u32::try_from(i + 1).unwrap_or(u32::MAX))
                    .with_size_of_set(
                        u32::try_from(streams.subscriptions.len()).unwrap_or(u32::MAX),
                    ),
            );
        }

        let mut nodes = Vec::with_capacity(items.len() + 4);
        nodes.push(list);
        nodes.extend(items);
        nodes.push(
            AccessNode::new(REVISION_TAG, AriaRole::Status).with_name(revision_line(revision)),
        );
        nodes.push(
            AccessNode::new(STREAMS_TAG, AriaRole::Status)
                .with_name(streams_line(streams.subscriptions.len())),
        );
        nodes.push(
            AccessNode::new(PUBLISHED_TAG, AriaRole::Status)
                .with_name(published_line(streams.published_total)),
        );
        nodes
    }
}

impl WidgetView for SubscribeView {
    type Renderer = HelloSubscribeRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve the socket path: the `PINION_SUBSCRIBE_SOCK` env override, or a
/// per-pid temp path for a bare `cargo run`.
fn resolve_socket_path() -> PathBuf {
    std::env::var_os(SOCK_ENV).map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "pinion-hello-subscribe-{}.sock",
                std::process::id()
            ))
        },
        PathBuf::from,
    )
}

fn main() {
    let sock_path = resolve_socket_path();
    *SOCKET_PATH.lock().unwrap() = Some(sock_path.display().to_string());

    // Mount the socket transport over the shell's own ingress, unwrapped: this
    // example tracks nothing per connection of its own. The subscription
    // registry is the framework's (`pinion_rpc::process_registry`), and the
    // shell's `ProxyRpcIngress::on_disconnect` already releases a vanished
    // client's streams — which is the point: a consumer gets crash-safe stream
    // cleanup without writing any.
    let config = ShellConfig::new().on_rpc_ingress(move |ingress: Arc<dyn RpcIngress>| {
        let control = UnixSocketTransport::serve(&sock_path, ingress)
            .expect("bind hello-subscribe RPC socket");
        *CONTROL.lock().unwrap() = Some(control);
    });

    run_with_config::<SubscribeView>(config);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;
    use pinion_rpc::SubscriptionView;

    fn find_container<'a>(scene: &'a Scene, tag: &str) -> Option<&'a ContainerNode> {
        match scene {
            Scene::Container(c) if c.tag.as_deref() == Some(tag) => Some(c),
            Scene::Container(c) => c.children.iter().find_map(|ch| find_container(ch, tag)),
            _ => None,
        }
    }

    fn text_under(scene: &Scene, tag: &str) -> Option<String> {
        let container = find_container(scene, tag)?;
        container.children.iter().find_map(|ch| match ch {
            Scene::Text(t) => Some(t.content.clone()),
            Scene::Container(_) => text_under(ch, tag),
            _ => None,
        })
    }

    fn streams(views: Vec<SubscriptionView>, published: u64) -> SubscriptionsOutcome {
        SubscriptionsOutcome {
            subscriptions: views,
            published_total: published,
        }
    }

    fn stream(subscription: u64, conn: u64, delivered: u64) -> SubscriptionView {
        SubscriptionView {
            subscription,
            conn,
            revision: delivered,
            delivered_count: delivered,
            armed: true,
        }
    }

    #[test]
    fn the_status_lines_are_singular_and_plural() {
        assert_eq!(streams_line(0), "0 live change streams");
        assert_eq!(streams_line(1), "1 live change stream");
        assert_eq!(streams_line(3), "3 live change streams");
        assert_eq!(published_line(1), "1 notification published");
        assert_eq!(published_line(0), "0 notifications published");
    }

    #[test]
    fn the_revision_line_is_the_shared_ssot() {
        // The value a `scene/changed` notification carries, painted. The demo
        // asserts the two agree, so a divergence here is a divergence there.
        assert_eq!(revision_line(0), "Scene revision: 0");
        assert_eq!(revision_line(42), "Scene revision: 42");
    }

    #[test]
    fn a_stream_row_names_its_subscription_and_its_connection() {
        assert_eq!(
            stream_row_text(2, 7, 5),
            "stream #2 on conn #7 — 5 delivered"
        );
    }

    #[test]
    fn an_empty_panel_shows_zero_and_a_placeholder() {
        let owner = Owner::new();
        let scene = owner.run(|| subscribe_panel_scene(0, &streams(Vec::new(), 0)));
        assert_eq!(
            text_under(&scene, STREAMS_TAG).as_deref(),
            Some("0 live change streams"),
        );
        assert!(
            text_under(&scene, &row_tag(0))
                .as_deref()
                .is_some_and(|t| t.contains("No streams")),
            "empty panel shows the placeholder row",
        );
    }

    #[test]
    fn the_panel_renders_every_live_stream_with_its_owner() {
        let owner = Owner::new();
        let scene = owner
            .run(|| subscribe_panel_scene(9, &streams(vec![stream(1, 4, 2), stream(2, 5, 0)], 2)));
        assert_eq!(
            text_under(&scene, REVISION_TAG).as_deref(),
            Some("Scene revision: 9"),
        );
        assert_eq!(
            text_under(&scene, STREAMS_TAG).as_deref(),
            Some("2 live change streams"),
        );
        assert_eq!(
            text_under(&scene, PUBLISHED_TAG).as_deref(),
            Some("2 notifications published"),
        );
        assert_eq!(
            text_under(&scene, &row_tag(0)).as_deref(),
            Some("stream #1 on conn #4 — 2 delivered"),
        );
        assert_eq!(
            text_under(&scene, &row_tag(1)).as_deref(),
            Some("stream #2 on conn #5 — 0 delivered"),
        );
    }
}
