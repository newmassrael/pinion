//! `hello-endpoint-identity` — R1478 §5.7: a fixed-path RPC endpoint is an
//! **identity**, not a last-writer-wins slot.
//!
//! ## Why this exists
//!
//! R1469 (PINION-PR48) landed the endpoint's *exposure* at bind and, in
//! passing, found a neighbouring defect it deliberately left out of scope: the
//! transport unlinked whatever sat at the socket path before binding its own.
//! On a *stale* path — a socket file a crashed run left behind — that is the
//! whole point, and a fixed-path endpoint has to be re-bindable across
//! restarts. On a *live* path it is a silent takeover:
//!
//! * every later client reaching that path is served by the newcomer, while
//!   the incumbent keeps a listener no one can ever reach again;
//! * nothing in either process reports it — the incumbent's socket variable is
//!   still `Ok`, its accept loop still polls, and its clients simply stop
//!   arriving.
//!
//! The same missing invariant had a second half at teardown: a departing
//! endpoint removed *the path*, not *the socket file it bound*, so it could
//! delete a successor's endpoint on its way out.
//!
//! R1478 states the invariant once — **an endpoint owns a name, and only ever
//! binds or unbinds its own** — and this example is its reference consumer.
//!
//! ## What this demonstrates
//!
//! The binding mounts [`UnixSocketTransport`] on the `on_rpc_ingress` seam and,
//! crucially, does **not** `expect` the bind: a refusal is a state the app
//! reflects into the scene as data (§2 #7) on the `endpoint_state`
//! `role=status` region, alongside the instance's own `endpoint_label`.
//!
//! That is the §2 #7 argument in its sharpest form. An app that could not take
//! its RPC endpoint is exactly the app an agent cannot reach over that
//! endpoint — so "why is nobody answering?" has to be answerable *somewhere
//! else*. Here the out-of-band stdin channel answers it with structured data:
//! which path is contested, and that it is contested rather than missing. A
//! plain socket library's answer to that question is an errno on stderr, if the
//! process bothered to log one; here it is queryable on the live app.
//!
//! The label is what makes the takeover measurable rather than argued: two
//! instances aimed at one path render different labels, so a client connecting
//! to the socket learns *which* process owns it by reading the snapshot back.
//!
//! ## Verification
//!
//! `tools/demos/r1478_endpoint_identity.py`: instance `alpha` binds a path and
//! answers a real `AF_UNIX` client with its own label; instance `beta`, booted
//! at the *same* path while alpha lives, reports the contested bind and leaves
//! the socket file's inode untouched — and that same client is still served by
//! alpha; beta's exit does not take alpha's name with it; and a fresh instance
//! reclaims a genuinely stale path, so the refusal is ownership, not poisoning.
//!
//! The bind protocol itself (probe-on-`EADDRINUSE`, and the teardown guard) is
//! pinned deterministically by `pinion-rpc-transport`'s `unix_socket_roundtrip`
//! integration tests.

use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::{Arc, OnceLock};

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextAlign, TextStyle,
};
use pinion_core::theme::{ColorRole, use_theme};
use pinion_core::widget_core::{ExtraExternal, PrimarySurface};
use pinion_core::{External, Frame, Scene, WidgetCore};
use pinion_rpc::RpcIngress;
use pinion_rpc_transport::{TransportControl, UnixSocketTransport};
use pinion_shell::{ShellConfig, SizeStrategy, WidgetView, run_with_config, vello_renderer_impl};

// pinion-forge codegen output — defines `HelloEndpointIdentityRenderer` +
// `HelloEndpointIdentityRendererError` (the Vello wrapper).
include!(concat!(env!("OUT_DIR"), "/app.rs"));

vello_renderer_impl!(
    HelloEndpointIdentityRenderer,
    HelloEndpointIdentityRendererError
);

const WIN_W: u32 = 520;
const WIN_H: u32 = 260;

/// Shared `ThemeProvider` cache key (the `"app"` gallery convention).
const THEME_TAG: &str = "app";

const LABEL_TAG: &str = "endpoint_label";
const STATE_TAG: &str = "endpoint_state";
const HINT_TAG: &str = "endpoint_hint";

/// Env var naming the socket path to bind. The r1478 demo points two instances
/// at one path deliberately; a bare `cargo run` falls back to a per-pid temp
/// path, which no other process is contending for.
const SOCK_ENV: &str = "PINION_ENDPOINT_IDENTITY_SOCK";

/// Env var naming this instance, so two processes aimed at one path are
/// distinguishable *through the endpoint itself*: a client that connects to the
/// socket and reads the snapshot back learns which process owns the name.
const LABEL_ENV: &str = "PINION_ENDPOINT_IDENTITY_LABEL";

// ─── what became of this process's claim on its path ────────────────────────

/// R1478 — the outcome of this process's attempt to own its endpoint path.
///
/// Three states rather than a `Result<_, String>` because the *contested* case
/// is the one an operator and an agent must be able to act on, and it is not
/// the same event as "the parent directory does not exist". Collapsing them
/// into one error string would render identically and mean two different
/// things.
enum EndpointClaim {
    /// The bind succeeded: this process owns the name at its path.
    Bound,
    /// R1478 — the path is held by a **live** endpoint, so the bind was
    /// refused rather than allowed to displace it.
    Contested,
    /// The bind failed for an unrelated reason (permission, missing parent
    /// directory, path too long). Carries the OS's own words.
    Unavailable(String),
}

impl EndpointClaim {
    /// Classify a bind result. `AddrInUse` is precisely R1478's refusal — the
    /// transport only produces it when a liveness probe answered — so the
    /// distinguished state is read off the error kind rather than parsed out
    /// of its message.
    fn of(result: &io::Result<TransportControl>) -> Self {
        match result {
            Ok(_) => Self::Bound,
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => Self::Contested,
            Err(e) => Self::Unavailable(e.to_string()),
        }
    }
}

/// The process-global endpoint board: what this instance is called, where it
/// aimed, how the claim went, and the live control if it won one.
///
/// A global is the honest home, as in `hello-conn-lifecycle`: the transport is
/// mounted on the `on_rpc_ingress` hook, before the first `Owner` scope exists,
/// so a cache hook cannot reach it.
struct EndpointBoard {
    label: OnceLock<String>,
    path: OnceLock<String>,
    claim: OnceLock<EndpointClaim>,
    /// Parked for the process lifetime — the app owns its endpoint, and
    /// dropping this would unbind the socket.
    control: Mutex<Option<TransportControl>>,
}

static BOARD: EndpointBoard = EndpointBoard {
    label: OnceLock::new(),
    path: OnceLock::new(),
    claim: OnceLock::new(),
    control: Mutex::new(None),
};

impl EndpointBoard {
    fn label(&self) -> &str {
        self.label.get().map_or("unnamed", String::as_str)
    }

    fn path(&self) -> &str {
        self.path.get().map_or("(unresolved)", String::as_str)
    }

    fn claim(&self) -> Option<&EndpointClaim> {
        self.claim.get()
    }
}

// ─── scene-as-data text (one SSOT per line, shared by paint and a11y) ────────

/// The instance line — the identity a socket client reads back to learn which
/// process is behind the name.
fn label_line(label: &str) -> String {
    format!("Instance: {label}")
}

/// The `role=status` claim line. The single SSOT for the visible text and the
/// accessible name, so an assistive technology and an agent are told the same
/// thing about the same endpoint.
fn claim_line(claim: Option<&EndpointClaim>, path: &str) -> String {
    match claim {
        None => "Endpoint: not yet claimed".to_owned(),
        Some(EndpointClaim::Bound) => format!("Endpoint: bound at {path}"),
        Some(EndpointClaim::Contested) => {
            format!("Endpoint: refused — {path} is held by a live endpoint")
        }
        Some(EndpointClaim::Unavailable(why)) => format!("Endpoint: unavailable — {why}"),
    }
}

/// The human-facing hint under the panel.
fn hint_line() -> String {
    format!(
        "{SOCK_ENV} selects the path, {LABEL_ENV} names this instance.\n\
         A second instance aimed at a live path is refused, not granted it."
    )
}

// ─── the panel ──────────────────────────────────────────────────────────────

/// One tagged, horizontally-centred line of text.
///
/// Deliberately local to this binding, exactly as `hello-conn-lifecycle`'s
/// twin is: the "centred row" *vocabulary* is common across the example
/// gallery, but a shared helper for it is a substrate decision of its own and
/// not a side effect of this round. (R1478 obligation-3b note: the two copies
/// differ — this one takes `&str` and wraps its own `TextStyle` opinions
/// per-call — and two is not the lift trigger.)
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

/// Pure panel builder: `(label, path, claim) -> Scene`, resolving the theme
/// from the active owner scope. Split out so tests drive it with explicit
/// inputs instead of mutating the process-global [`BOARD`].
fn endpoint_panel_scene(label: &str, path: &str, claim: Option<&EndpointClaim>) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();

    let title = Scene::Text(TextNode::styled(
        "RPC endpoint identity (socket transport)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let label_row = tagged_centered_line(
        LABEL_TAG,
        label_line(label),
        TextStyle::new()
            .with_size_px(14)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    );

    // A claim this process did not win is Error-toned, so "I do not own my
    // endpoint" is legible in the pixels as well as in the data.
    let claim_fg = if matches!(claim, Some(EndpointClaim::Bound)) {
        theme.resolve(ColorRole::OnSurfaceMuted)
    } else {
        theme.resolve(ColorRole::Error)
    };
    let claim_row = tagged_centered_line(
        STATE_TAG,
        claim_line(claim, path),
        TextStyle::new().with_size_px(14).with_fg(claim_fg),
    );

    let hint = tagged_centered_line(
        HINT_TAG,
        hint_line(),
        TextStyle::new()
            .with_size_px(12)
            .with_fg(theme.resolve(ColorRole::OnSurfaceMuted))
            .with_align(TextAlign::Center),
    );

    Scene::Container(
        ContainerNode::new(vec![title, label_row, claim_row, hint])
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

/// view-fn (§6.3): pure sync `() -> Scene`, reading the endpoint board.
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(_state: (), _frame: &Frame) -> Scene {
    endpoint_panel_scene(BOARD.label(), BOARD.path(), BOARD.claim())
}

/// The binding. Hand-written (not `#[widget]`-derived) because it is
/// display-only: its whole subject is a boot-time claim, not a pointer input.
/// PR-51's `primary_surface()` opt-out is exactly this shape.
struct EndpointIdentityView;

impl WidgetCore for EndpointIdentityView {
    type State = ();
    type Event = ();

    /// (PR-51) No primary surface: no statechart, no captured gesture.
    fn primary_surface() -> Option<PrimarySurface> {
        None
    }

    fn create_external() -> Box<dyn External> {
        unreachable!("hello-endpoint-identity has no primary surface — see primary_surface()")
    }

    fn tag() -> &'static str {
        unreachable!("hello-endpoint-identity has no primary surface — see primary_surface()")
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
        "pinion hello-endpoint-identity (R1478 §5.7)"
    }

    fn fmt_state_log(_state: &Self::State) -> String {
        "display-only (the state is this process's boot-time endpoint claim)".to_owned()
    }
}

impl WidgetA11y for EndpointIdentityView {
    /// The claim is a `role=status` live region: an app that could not take its
    /// endpoint should *announce* that, and the name comes from the same SSOT
    /// the paint uses, so AT and the pixels cannot drift apart.
    fn access_node(_state: &Self::State, _focused: Option<&str>) -> Vec<AccessNode> {
        vec![
            AccessNode::new(STATE_TAG, AriaRole::Status)
                .with_name(claim_line(BOARD.claim(), BOARD.path())),
            AccessNode::new(LABEL_TAG, AriaRole::Status).with_name(label_line(BOARD.label())),
        ]
    }
}

impl WidgetView for EndpointIdentityView {
    type Renderer = HelloEndpointIdentityRenderer;

    fn initial_size_strategy() -> SizeStrategy {
        SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

/// Resolve the socket path: the [`SOCK_ENV`] override, or a per-pid temp path
/// for a bare `cargo run` (which is uncontended by construction).
fn resolve_socket_path() -> PathBuf {
    std::env::var_os(SOCK_ENV).map_or_else(
        || {
            std::env::temp_dir().join(format!(
                "pinion-hello-endpoint-identity-{}.sock",
                std::process::id()
            ))
        },
        PathBuf::from,
    )
}

/// This instance's name, defaulting to its pid so two bare `cargo run`s are
/// still distinguishable.
fn resolve_label() -> String {
    std::env::var(LABEL_ENV)
        .ok()
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("pid-{}", std::process::id()))
}

fn main() {
    let sock_path = resolve_socket_path();
    let _ = BOARD.label.set(resolve_label());
    let _ = BOARD.path.set(sock_path.display().to_string());

    let config = ShellConfig::new().on_rpc_ingress(move |ingress: Arc<dyn RpcIngress>| {
        // R1478 — deliberately NOT `expect`ed. A refused bind is a state this
        // app reports, not a reason to die: the process that lost the name is
        // exactly the one an agent has to ask over the other channel.
        let result = UnixSocketTransport::serve(&sock_path, ingress);
        let _ = BOARD.claim.set(EndpointClaim::of(&result));
        *BOARD.control.lock().unwrap() = result.ok();
    });

    run_with_config::<EndpointIdentityView>(config);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_claim_lines_name_three_distinct_states() {
        // The contested case must not read like a generic failure: an operator
        // seeing "unavailable" looks at permissions, one seeing "held by a
        // live endpoint" looks for the other process.
        let bound = claim_line(Some(&EndpointClaim::Bound), "/run/app.sock");
        let contested = claim_line(Some(&EndpointClaim::Contested), "/run/app.sock");
        let unavailable = claim_line(
            Some(&EndpointClaim::Unavailable("permission denied".to_owned())),
            "/run/app.sock",
        );

        assert!(bound.contains("bound at /run/app.sock"));
        assert!(contested.contains("held by a live endpoint"));
        assert!(unavailable.contains("permission denied"));
        assert_ne!(bound, contested);
        assert_ne!(contested, unavailable);
        // The contested line names the path, which is the actionable half.
        assert!(contested.contains("/run/app.sock"));
    }

    #[test]
    fn a_contested_bind_is_classified_from_the_error_kind() {
        // R1478 — the transport reports a live incumbent as `AddrInUse` and
        // nothing else does, so the distinguished state is read off the kind.
        // Parsing the message instead would break the moment the wording
        // changed, and every test here would still pass.
        let contested: io::Result<TransportControl> =
            Err(io::Error::new(io::ErrorKind::AddrInUse, "held"));
        let other: io::Result<TransportControl> =
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope"));

        assert!(matches!(
            EndpointClaim::of(&contested),
            EndpointClaim::Contested
        ));
        assert!(matches!(
            EndpointClaim::of(&other),
            EndpointClaim::Unavailable(_)
        ));
    }

    #[test]
    fn an_unclaimed_board_still_renders_a_line() {
        // The view runs before the ingress hook has fired on some boot
        // orderings; it must render a state rather than an empty region, or an
        // agent polling early would read "no such tag" and conclude the app
        // has no endpoint story at all.
        let line = claim_line(None, "/run/app.sock");
        assert!(!line.is_empty());
        assert!(line.contains("not yet claimed"));
    }
}
