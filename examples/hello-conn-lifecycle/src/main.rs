//! `hello-conn-lifecycle` — R1393 §5.7 PINION-PR67: the reference consumer of
//! the **RPC connection identity + lifecycle** seam.
//!
//! ## Why this exists (the sprag `list-clients` / `no-detached` case)
//!
//! Before R1393 a `RpcIngress` saw only `submit(frame)`, and an `RpcFrame`
//! carried only `{ request, reply }`: a *stateful* ingress — one dispatch
//! owner serving many connections — could not tell which connection a frame
//! came from, nor when a connection closed. So it could not keep per-connection
//! state crash-safely. sprag's motivating deliverable is exactly this: tmux's
//! `detach-on-destroy no-detached` and a `list-clients` "N viewers" badge need
//! a per-session count of *live* attachments, decremented however a client
//! goes away (including a crash, when no explicit `detach` ever arrives).
//!
//! R1393 added the two missing edges (identical two-item scope PINION-PR67
//! asked for, nothing more):
//!
//! * **R-67.1** — every [`RpcFrame`] now carries a [`ConnId`], a process-unique
//!   opaque lifecycle token the transport stamps on each frame, so a stateful
//!   ingress attributes the frame to its originating connection.
//! * **R-67.2** — [`RpcIngress`] gained [`RpcIngress::on_connect`] /
//!   [`RpcIngress::on_disconnect`] (default no-op, so every existing ingress is
//!   unchanged). The Unix-socket transport fires `on_disconnect` whenever a
//!   connection's reader thread ends — EOF, reset, or crash — which is the
//!   **crash-safe cleanup hook**: however the client dies, the reader ends and
//!   the count is decremented.
//!
//! ## What this demonstrates
//!
//! A stateful [`AttachTrackingIngress`] wraps the shell's real ingress and
//! mounts the [`UnixSocketTransport`]. Its `submit` forwards frames to the real
//! dispatch path unchanged (so a socket client still drives + queries the app);
//! its `on_connect` / `on_disconnect` maintain a live-connection set. The view
//! reflects that set into the scene as data (§2 #7): a `role=status` line "N
//! connections attached" and a `list` of the live opaque [`ConnId`] values. An
//! AI agent (or the r1393 demo) opens/closes socket connections and reads the
//! count + ids back with **no pixels** — the whole PINION-PR67 round-trip over
//! the §5.12 plane.
//!
//! The built-in stdin transport is deliberately NOT wrapped, so its own
//! connection does not count: it is the out-of-band *observer* channel (the
//! demo queries `scene/snapshot` over it), while the socket connections are the
//! tracked *clients* — mirroring sprag, where the tracked attachments are the
//! pane/session clients, not the control channel.
//!
//! ## R1469 PINION-PR48 — the endpoint's boot exposure
//!
//! A second consumer requirement landed on the same mount site. A host whose
//! policy is `APP_RPC=off` wants the socket **bound but withdrawn**: the path
//! is always there, service is refused. Before PR-48 the only expressible
//! sequence was `serve()` (which binds *serving* and arms the accept loop)
//! followed by a withdraw once the control came back — so every such consumer
//! had a window, and a client landing in it was admitted for its whole
//! session, because withdrawing does not evict live connections.
//!
//! This example is that consumer: [`EXPOSURE_ENV`] declares the boot
//! [`Exposure`], and `main` hands it to
//! [`UnixSocketTransport::serve_with_exposure`] rather than applying it after.
//! The exposure is then reflected into the scene as data (§2 #7) on the
//! `conn_exposure` region, read live off the [`TransportControl`] — so an agent
//! learns "bound, refusing service" over the *out-of-band* stdin channel, the
//! one channel a withdrawn endpoint leaves open. A plain socket library has no
//! answer to that question at all.
//!
//! ## Verification
//!
//! `tools/demos/r1469_transport_exposure.py`: the same binary booted twice —
//! withdrawn, where the socket file exists but a client is refused and the
//! count never leaves 0; then serving, where that same client attaches.
//!
//! `tools/demos/r1393_conn_lifecycle.py`: over stdin the count starts at 0; a
//! raw `AF_UNIX` connection opens → the count rises to 1 and the new id appears;
//! a second opens → 2 distinct ids; closing one drops the count to 1 and leaves
//! the *other* id (attribution: the right connection was cleaned up); closing
//! the last drops it to 0. The socket transport's own lifecycle hooks are pinned
//! by `pinion-rpc-transport`'s `unix_socket_roundtrip` integration tests.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextAlign, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::use_repaint_sink;
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{External, Frame, RepaintSink, Scene, WidgetCore};
use pinion_rpc::{ConnId, RpcFrame, RpcIngress};
use pinion_rpc_transport::{Exposure, TransportControl, UnixSocketTransport};
use pinion_shell::{ShellConfig, SizeStrategy, WidgetView, run_with_config, vello_renderer_impl};

// pinion-forge codegen output — defines `HelloConnLifecycleRenderer` +
// `HelloConnLifecycleRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(HelloConnLifecycleRenderer, HelloConnLifecycleRendererError);

const WIN_W: u32 = 460;
/// R1469 — grown by one row's worth over R1393's 380 to seat the
/// endpoint-exposure line without crowding the list.
const WIN_H: u32 = 420;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

const STATUS_TAG: &str = "conn_status";
const LIST_TAG: &str = "conn_list";
const HINT_TAG: &str = "conn_hint";
const EXPOSURE_TAG: &str = "conn_exposure";

/// Env var naming the socket path to bind. The r1393 demo sets a unique path;
/// a bare `cargo run` falls back to a per-pid temp path (printed in the hint).
const SOCK_ENV: &str = "PINION_CONN_LIFECYCLE_SOCK";

/// R1469 §5.7 PINION-PR48 — env var declaring the endpoint's **boot
/// [`Exposure`]**, the deployment knob a host exposes as `APP_RPC=off`:
/// `withdrawn` binds the socket but refuses service, anything else (including
/// unset) serves. Read once, in `main`, and handed to the bind — never applied
/// after it, which is the whole point of PR-48.
const EXPOSURE_ENV: &str = "PINION_CONN_LIFECYCLE_EXPOSURE";

// ─── the process-global connection board ────────────────────────────────────

/// The live-connection board, shared by the tracking ingress (mounted in the
/// `on_rpc_ingress` hook, before any `Owner` scope exists) and the view (reads
/// it each frame / snapshot).
///
/// A global is the honest home here — unlike `hello-live-data`'s
/// `Owner::cache` producer, the socket transport is mounted *before* the first
/// `Owner` scope, so a cache hook cannot reach it. A single-window example has
/// exactly one board.
struct ConnBoard {
    /// Raw values of the currently-open tracked socket connections, one per
    /// live connection. `BTreeSet` keeps the rendered id list deterministic
    /// (sorted) regardless of connect/disconnect interleaving.
    ids: Mutex<BTreeSet<u64>>,
    /// The shell's repaint sink, seeded once at boot ([`use_repaint_sink`]) so a
    /// connect/disconnect on a transport thread wakes the window to refresh the
    /// badge. `None` until boot seeds it — a connection arriving before the
    /// first paint still updates `ids`; the boot paint reads it anyway.
    sink: Mutex<Option<Arc<dyn RepaintSink>>>,
    /// The socket transport's lifetime handle, parked here for the process
    /// lifetime (the app owns its endpoint; dropping it would unbind the
    /// socket).
    control: Mutex<Option<TransportControl>>,
}

static BOARD: ConnBoard = ConnBoard {
    ids: Mutex::new(BTreeSet::new()),
    sink: Mutex::new(None),
    control: Mutex::new(None),
};

/// The resolved socket path, for the human-facing hint line. Seeded in `main`.
static SOCKET_PATH: Mutex<Option<String>> = Mutex::new(None);

impl ConnBoard {
    /// Wake the shell to repaint if a sink has been seeded (a no-op before
    /// boot). Locks `sink` only after the `ids` guard is dropped by the caller,
    /// so there is no nested lock.
    fn wake(&self) {
        if let Some(sink) = self.sink.lock().unwrap().as_ref() {
            sink.request_repaint();
        }
    }

    /// The live ids, sorted — the SSOT the view and a11y both read.
    fn snapshot_ids(&self) -> Vec<u64> {
        self.ids.lock().unwrap().iter().copied().collect()
    }

    /// R1469 — the endpoint's exposure *right now*, read off the live
    /// [`TransportControl`], or `None` before one is mounted. Reading the
    /// control rather than caching the boot value is what makes the rendered
    /// line answer the question actually asked ("is this endpoint serving?")
    /// rather than a stale "what did it boot as?".
    fn exposure(&self) -> Option<Exposure> {
        self.control
            .lock()
            .unwrap()
            .as_ref()
            .map(TransportControl::exposure)
    }
}

// ─── the stateful tracking ingress ──────────────────────────────────────────

/// A stateful [`RpcIngress`] that forwards every frame to the real dispatch
/// path unchanged while maintaining the live-connection [`ConnBoard`] through
/// the R-PR67 lifecycle hooks. This is the whole point of PINION-PR67: the same
/// dispatch owner serves many socket connections and can now track each one.
struct AttachTrackingIngress {
    inner: Arc<dyn RpcIngress>,
}

impl RpcIngress for AttachTrackingIngress {
    fn submit(&self, frame: RpcFrame) {
        // Tracking is purely additive: the frame (with its `conn` + reply)
        // flows to the real core, and its response still routes back to the
        // originating socket.
        self.inner.submit(frame);
    }

    fn on_connect(&self, conn: ConnId) {
        BOARD.ids.lock().unwrap().insert(conn.get());
        BOARD.wake();
    }

    fn on_disconnect(&self, conn: ConnId) {
        // Fires however the client went away (EOF / reset / crash) — the
        // crash-safe decrement no explicit `detach` message could guarantee.
        BOARD.ids.lock().unwrap().remove(&conn.get());
        BOARD.wake();
    }
}

// ─── scene-as-data rendering (pure; the view is a thin wrapper) ──────────────

/// The `role=status` count text — the single SSOT for the visible line and its
/// accessible name.
fn status_line(count: usize) -> String {
    match count {
        0 => "0 connections attached".to_owned(),
        1 => "1 connection attached".to_owned(),
        n => format!("{n} connections attached"),
    }
}

/// One live connection's row text — the SSOT for the painted row and its
/// `listitem` accessible name. The id is opaque; only its identity matters.
fn conn_row_text(id: u64) -> String {
    format!("conn #{id}")
}

/// Stable per-row tag shared by the painted row and its `listitem`
/// `AccessNode`, so AT bounds attach to the paint.
fn row_tag(visible_index: usize) -> String {
    format!("conn_row_{visible_index}")
}

/// R1469 PINION-PR48 — the endpoint-exposure line: the single SSOT for the
/// painted text and its `role=status` accessible name.
///
/// This is the §2 #7 half of PR-48 that a plain socket library cannot offer:
/// "bound but refusing" is a real, distinct state, and here an agent reads it
/// as *scene data* over the out-of-band channel — the one channel still open
/// when the endpoint is withdrawn. `None` is the pre-mount state (unit tests
/// drive the pure builder), reported as such rather than guessed at.
fn exposure_line(exposure: Option<Exposure>) -> String {
    match exposure {
        Some(Exposure::Serving) => "Endpoint: serving".to_owned(),
        Some(Exposure::Withdrawn) => "Endpoint: withdrawn (bound, refusing service)".to_owned(),
        None => "Endpoint: unbound".to_owned(),
    }
}

/// The human-facing hint (socket path); empty in tests, where nothing seeded
/// [`SOCKET_PATH`].
fn socket_hint() -> String {
    match SOCKET_PATH.lock().unwrap().as_ref() {
        Some(path) => format!("Socket: {path}"),
        None => String::new(),
    }
}

/// One connection row — a tagged container holding the single SSOT line.
fn conn_row_scene(visible_index: usize, id: u64, theme: &pinion_core::Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            conn_row_text(id),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurface)),
        ))])
        .with_tag(row_tag(visible_index))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(WIN_W - 96, 26)),
        ),
    )
}

/// The connections panel — the live rows, or a placeholder when none.
/// `LIST_TAG` stays present across both states.
fn conn_list_scene(ids: &[u64], theme: &pinion_core::Theme) -> Scene {
    let children: Vec<Scene> = if ids.is_empty() {
        vec![Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                "No connections \u{2014} open the socket to attach".to_owned(),
                Rect::default(),
                TextStyle::new()
                    .with_size_px(14)
                    .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
                    .with_align(TextAlign::Center),
            ))])
            .with_tag(row_tag(0))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Row)
                    .with_justify(JustifyContent::Center)
                    .with_align_items(AlignItems::Center)
                    .with_size(Size::px(WIN_W - 96, 26)),
            ),
        )]
    } else {
        ids.iter()
            .enumerate()
            .map(|(i, id)| conn_row_scene(i, *id, theme))
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
                    .with_padding(Rect::new(12, 12, 12, 12))
                    .with_size(Size::px(WIN_W - 56, 210)),
            ),
    )
}

/// R1469 (obligation 3b) — one tagged, horizontally-centred line of text.
///
/// The panel's status / exposure / hint rows had byte-identical wiring and
/// differed only in their [`TextStyle`]; adding the exposure row made that the
/// third copy, so the wiring is shared and every style opinion stays at the
/// call site. Deliberately local to this binding: the surrounding "centred
/// row" *vocabulary* is common across the example gallery, but a shared
/// helper for it is a substrate decision of its own, not a side effect of
/// this round.
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

/// Pure panel builder: `(live ids, endpoint exposure) -> Scene`, resolving the
/// theme from the active owner scope. Split out so tests drive it with explicit
/// inputs instead of mutating the process-global [`BOARD`].
fn conn_panel_scene(ids: &[u64], exposure: Option<Exposure>) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(TextNode::styled(
        "Live RPC connections (socket transport)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let status = tagged_centered_line(
        STATUS_TAG,
        status_line(ids.len()),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
    );

    // R1469 PINION-PR48 — the endpoint's exposure, as data. Muted when
    // serving (the unremarkable case), Error-toned when withdrawn, so the
    // "bound but refusing" state is legible in the pixels too.
    let exposure_fg = if exposure == Some(Exposure::Withdrawn) {
        theme.resolve(ColorRole::Error)
    } else {
        theme.resolve(ColorRole::OnSurfaceMuted)
    };
    let exposure_row = tagged_centered_line(
        EXPOSURE_TAG,
        exposure_line(exposure),
        TextStyle::new().with_size_px(14).with_fg(exposure_fg),
    );

    let list = conn_list_scene(ids, &theme);

    let hint = tagged_centered_line(
        HINT_TAG,
        socket_hint(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
            .with_align(TextAlign::Center),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, exposure_row, list, hint])
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

/// view-fn (§6.3): pure sync `() -> Scene`, reading the live-connection board
/// (a producer-authoritative shared handle, like `hello-live-data`'s buffer).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    conn_panel_scene(&BOARD.snapshot_ids(), BOARD.exposure())
}

/// The binding. Hand-written (not `#[widget]`-derived) because it is
/// display-only: its sole "input" arrives out-of-band through the socket
/// transport's lifecycle hooks, not through a pointer. PR-51's
/// `primary_surface()` opt-out is exactly this shape (as `hello-chart-fill`).
struct ConnLifecycleView;

impl WidgetCore for ConnLifecycleView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: no statechart, no captured gesture — the
    /// connection lifecycle drives the scene, not an External.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-conn-lifecycle has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-conn-lifecycle has no primary surface — see primary_surface()")
    }

    /// Seed the repaint sink into the board at boot, inside the owner scope, so
    /// a connect/disconnect on a transport thread can wake the window. Spawns
    /// nothing (the socket transport is mounted in `main`'s ingress hook).
    fn create_extra_externals() -> Vec<ExtraExternal> {
        *BOARD.sink.lock().unwrap() = Some(use_repaint_sink());
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
        "pinion hello-conn-lifecycle (R1393 §5.7 PINION-PR67)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (input arrives through the socket transport's lifecycle)".to_owned()
    }
}

impl WidgetA11y for ConnLifecycleView {
    /// A WAI-ARIA `list` over the live connections (one `listitem` each, named
    /// from the same SSOT the paint uses) plus a `role=status` live region for
    /// the count.
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        let ids = BOARD.snapshot_ids();

        let mut list = AccessNode::new(LIST_TAG, AriaRole::List).with_name("Live connections");
        let mut items: Vec<AccessNode> = Vec::new();
        for (i, id) in ids.iter().enumerate() {
            let tag = row_tag(i);
            list = list.with_child(tag.clone());
            items.push(
                AccessNode::new(tag, AriaRole::ListItem)
                    .with_name(conn_row_text(*id))
                    .with_position_in_set(u32::try_from(i + 1).unwrap_or(u32::MAX))
                    .with_size_of_set(u32::try_from(ids.len()).unwrap_or(u32::MAX)),
            );
        }

        let mut nodes = Vec::with_capacity(items.len() + 3);
        nodes.push(list);
        nodes.extend(items);
        nodes.push(AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_line(ids.len())));
        // R1469 — the endpoint's exposure is a second live region: it changes
        // out-of-band (a runtime withdraw), so AT should announce it, and it is
        // named from the same SSOT the paint uses.
        nodes.push(
            AccessNode::new(EXPOSURE_TAG, AriaRole::Status)
                .with_name(exposure_line(BOARD.exposure())),
        );
        nodes
    }
}

impl WidgetView for ConnLifecycleView {
    type Renderer = HelloConnLifecycleRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve the socket path: the `PINION_CONN_LIFECYCLE_SOCK` env override, or a
/// per-pid temp path for a bare `cargo run`.
fn resolve_socket_path() -> PathBuf {
    std::env::var_os(SOCK_ENV).map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "pinion-hello-conn-lifecycle-{}.sock",
                std::process::id()
            ))
        },
        PathBuf::from,
    )
}

/// R1469 PINION-PR48 — parse the boot [`Exposure`] from the raw env value.
///
/// Pure (takes the value, does not read the environment) so both arms are unit
/// testable without mutating process state. Only the exact token `withdrawn`
/// withdraws: an unrecognised value must not silently take an endpoint off the
/// air, so everything else — including unset — is [`Exposure::default`], which
/// is the pre-PR48 behaviour.
fn parse_boot_exposure(raw: Option<&str>) -> Exposure {
    match raw.map(str::trim) {
        Some(v) if v.eq_ignore_ascii_case("withdrawn") => Exposure::Withdrawn,
        _ => Exposure::default(),
    }
}

/// The boot exposure this process was launched with.
fn boot_exposure() -> Exposure {
    parse_boot_exposure(std::env::var(EXPOSURE_ENV).ok().as_deref())
}

fn main() {
    let sock_path = resolve_socket_path();
    *SOCKET_PATH.lock().unwrap() = Some(sock_path.display().to_string());
    // R-PR48 — resolved BEFORE the bind, because it is an argument to the
    // bind. Reading it here (rather than withdrawing after `serve` returned)
    // is exactly the one-line change PINION-PR48 asked for: there is no
    // instant at which this endpoint serves against its declared policy.
    let exposure = boot_exposure();

    // Mount the socket transport through the ingress hook, wrapping the shell's
    // real ingress in the stateful tracker. The built-in stdin transport is
    // installed separately by `run_with_config` over the UNwrapped ingress, so
    // the observer channel never counts — and it stays reachable even when the
    // socket is withdrawn, which is how an agent still reads this app's state.
    let config = ShellConfig::new().on_rpc_ingress(move |ingress| {
        let tracking: Arc<dyn RpcIngress> = Arc::new(AttachTrackingIngress { inner: ingress });
        let control = UnixSocketTransport::serve_with_exposure(&sock_path, tracking, exposure)
            .expect("bind hello-conn-lifecycle RPC socket");
        // Park the endpoint for the process lifetime — the app owns its socket.
        *BOARD.control.lock().unwrap() = Some(control);
    });

    run_with_config::<ConnLifecycleView>(config);
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::reactive::Owner;

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

    #[test]
    fn status_line_singular_and_plural() {
        assert_eq!(status_line(0), "0 connections attached");
        assert_eq!(status_line(1), "1 connection attached");
        assert_eq!(status_line(4), "4 connections attached");
    }

    #[test]
    fn conn_row_text_is_the_shared_ssot() {
        assert_eq!(conn_row_text(7), "conn #7");
        assert_eq!(conn_row_text(1042), "conn #1042");
    }

    #[test]
    fn boot_exposure_withdraws_only_on_the_exact_token() {
        // R1469 PINION-PR48 — an unrecognised value must not take the endpoint
        // off the air, so only `withdrawn` (case/space insensitive) withdraws.
        assert_eq!(parse_boot_exposure(Some("withdrawn")), Exposure::Withdrawn);
        assert_eq!(
            parse_boot_exposure(Some(" Withdrawn ")),
            Exposure::Withdrawn
        );
        assert_eq!(parse_boot_exposure(Some("serving")), Exposure::Serving);
        assert_eq!(parse_boot_exposure(None), Exposure::Serving);
        assert_eq!(parse_boot_exposure(Some("")), Exposure::Serving);
        assert_eq!(parse_boot_exposure(Some("off")), Exposure::Serving);
    }

    #[test]
    fn the_exposure_line_names_all_three_states() {
        // The SSOT the paint and the `role=status` accessible name share.
        assert_eq!(exposure_line(Some(Exposure::Serving)), "Endpoint: serving");
        assert_eq!(
            exposure_line(Some(Exposure::Withdrawn)),
            "Endpoint: withdrawn (bound, refusing service)",
        );
        assert_eq!(exposure_line(None), "Endpoint: unbound");
    }

    #[test]
    fn the_panel_reports_the_endpoint_exposure_as_data() {
        // R1469 §2 #7 — "bound but refusing" is legible without pixels.
        let owner = Owner::new();
        let withdrawn = owner.run(|| conn_panel_scene(&[], Some(Exposure::Withdrawn)));
        assert_eq!(
            text_under(&withdrawn, EXPOSURE_TAG).as_deref(),
            Some("Endpoint: withdrawn (bound, refusing service)"),
        );
        let serving = owner.run(|| conn_panel_scene(&[], Some(Exposure::Serving)));
        assert_eq!(
            text_under(&serving, EXPOSURE_TAG).as_deref(),
            Some("Endpoint: serving"),
        );
    }

    #[test]
    fn empty_panel_shows_zero_and_a_placeholder() {
        let owner = Owner::new();
        let scene = owner.run(|| conn_panel_scene(&[], Some(Exposure::Serving)));
        assert_eq!(
            text_under(&scene, STATUS_TAG).as_deref(),
            Some("0 connections attached"),
        );
        assert!(
            text_under(&scene, &row_tag(0))
                .as_deref()
                .is_some_and(|t| t.contains("No connections")),
            "empty panel shows the placeholder row",
        );
    }

    #[test]
    fn panel_renders_the_live_ids_in_order() {
        let owner = Owner::new();
        // Explicit ids — the pure panel builder never reads the global BOARD.
        let scene = owner.run(|| conn_panel_scene(&[3, 8], Some(Exposure::Serving)));
        assert_eq!(
            text_under(&scene, STATUS_TAG).as_deref(),
            Some("2 connections attached"),
        );
        assert_eq!(text_under(&scene, &row_tag(0)).as_deref(), Some("conn #3"));
        assert_eq!(text_under(&scene, &row_tag(1)).as_deref(), Some("conn #8"));
    }

    #[test]
    fn a11y_exposes_a_list_and_a_status_over_the_live_ids() {
        // The ONE test that touches the process-global BOARD (all others use
        // the pure builder with explicit ids), so no cross-test race on it.
        BOARD.ids.lock().unwrap().clear();
        let tracker = AttachTrackingIngress {
            // A never-fired inner: on_connect/on_disconnect never call submit.
            inner: Arc::new(NoopIngress),
        };
        // The lifecycle hooks maintain the board crash-safely. Capture the two
        // opaque ids (monotonic: `a < b`) so we can close a specific one.
        let a = ConnId::allocate();
        let b = ConnId::allocate();
        tracker.on_connect(a);
        tracker.on_connect(b);
        assert_eq!(
            BOARD.snapshot_ids(),
            vec![a.get(), b.get()],
            "two opens tracked"
        );

        let owner = Owner::new();
        let nodes = owner.run(|| <ConnLifecycleView as WidgetA11y>::access_node(&(), None));
        let list = nodes.iter().find(|n| n.tag == LIST_TAG).expect("list node");
        assert_eq!(list.role, AriaRole::List);
        assert_eq!(list.children.len(), 2, "two live connections claimed");
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.role == AriaRole::ListItem)
                .count(),
            2,
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status)
        );

        // Closing one drops it, and leaves exactly the other id — attribution.
        tracker.on_disconnect(a);
        assert_eq!(BOARD.snapshot_ids(), vec![b.get()], "the right id survived");

        // Clean up the global for any later run in the same process.
        BOARD.ids.lock().unwrap().clear();
    }

    /// A do-nothing inner ingress for the a11y test (its `submit` is never
    /// reached — only the lifecycle hooks run).
    struct NoopIngress;
    impl RpcIngress for NoopIngress {
        fn submit(&self, _frame: RpcFrame) {}
    }
}
