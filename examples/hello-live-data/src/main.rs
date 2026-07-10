//! `hello-live-data` — R1000 §5.23: the first framework consumer of the
//! **off-thread external-async-data → repaint seam** (`RepaintSink`).
//!
//! ## Why this exists (the sprag PR-3 reference consumer)
//!
//! Every prior example's data either lives in `State` (a `Copy` snapshot) or
//! is fetched on the UI thread through the `LocalTaskPump`. A terminal pane —
//! sprag's motivating case (R969) — is different: a child process writes its
//! PTY from a **separate OS thread**, and the binding must (a) read that
//! producer-authoritative `Send` data in `view` each frame and (b) repaint
//! when it changes, *without* owning the event loop. R999 added the one
//! missing edge — `RepaintSink` (obtained via [`use_repaint_sink`]) — on top
//! of the existing `Owner::cache` data substrate. This is the in-repo
//! reference consumer proving that seam end to end through a real binding.
//!
//! ## What this demonstrates
//!
//! A live event log. A background producer thread (the PTY-reader analog) owns
//! a `Send` shared buffer (`Arc<Mutex<Vec<String>>>`); each time it is poked it
//! appends a line and calls `RepaintSink::request_repaint`. The **Tick**
//! button pokes the producer (the stdin → child analog). `view` reads the
//! shared buffer directly each frame (like a producer-authoritative grid),
//! renders the lines as a WAI-ARIA `list`, and shows the count in a
//! `role=status` line.
//!
//! ## Architecture (producer-authoritative, off-thread, event-driven)
//!
//! - The buffer + producer thread + poke channel are self-created once in a
//!   `use_live_log` `Owner::cache` hook (the `use_storage` / R923 pattern), so
//!   nothing flows through `main()`. The thread is spawned in
//!   `create_extra_externals` (boot), never in the pure `view`.
//! - `use_repaint_sink()` is pre-resolved *before* the cache factory (the R666
//!   nested-`Owner::cache` guard) and handed to the producer thread.
//! - The reducer ([`LiveDataView::update`]) only pokes the producer channel;
//!   it never touches the buffer. The producer thread appends + wakes; the
//!   next frame's `view` re-reads the buffer. Event-driven, no polling.
//!
//! ## Verification
//!
//! `tools/demos/r1000_live_data.py`: boot (empty log) → Tick → a background
//! line appears (the producer thread wrote it and woke the shell) → Tick again
//! → two lines; the count `role=status` + `list` / `listitem` roles throughout
//! — all observed as scene-as-data (§2 #7), no pixels.

use pinion_a11y::{AccessNode, AriaRole, WidgetA11y};
use pinion_core::external::External;
use pinion_core::reactive::Owner;
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, FlexDirection, JustifyContent, LayoutStyle, Size, TextAlign, TextStyle,
};
use pinion_core::theme::{ColorRole, Theme, use_theme};
use pinion_core::use_repaint_sink;
use pinion_core::widget_core::ExtraExternal;
use pinion_core::widgets::aria::apply_aria_activate;
use pinion_core::widgets::button::{ButtonExternal, ButtonState};
use pinion_core::{Frame, Scene, WidgetCore};
use pinion_shell::use_waiter_registry;
use pinion_shell::{WidgetView, vello_renderer_impl};
use pinion_widget_paint::button::{
    ButtonColors, ButtonStyle, button_a11y_state, read_button_focused, read_button_state,
};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(HelloLiveDataRenderer, HelloLiveDataRendererError);

const WIN_W: u32 = 440;
const WIN_H: u32 = 360;
const THEME_TAG: &str = "app";

const TICK_TAG: &str = "tick";
const TICK_HOVER_KEY: &str = "tick_hover";
const STATUS_TAG: &str = "log_status";
const LIST_TAG: &str = "log_list";

/// `Owner::cache` key for the shared producer state (buffer + poke channel +
/// retained thread handle).
const LOG_KEY: &str = "live_data.log";

/// Max lines kept resident (a real log tailer windows; this keeps the demo
/// scene bounded). Older lines fall off the front.
const MAX_LINES: usize = 8;

/// The descriptor the producer thread writes for poke `seq` — the single SSOT
/// shared by the painted [`TextNode`] and the `listitem` accessible name.
fn producer_line(seq: usize) -> String {
    format!("[{seq:03}] background event #{seq}")
}

/// Stable per-row tag shared by the painted row and its `listitem`
/// `AccessNode`, so AT bounds attach to the paint.
fn row_tag(visible_index: usize) -> String {
    format!("log_row_{visible_index}")
}

// ─── producer-authoritative shared state (buffer + thread + poke channel) ───

/// The off-thread producer's shared state. The `Arc<Mutex<Vec<String>>>` buffer
/// is written by the producer thread and read by `view`/`access_node` on the UI
/// thread (producer-authoritative, R969); `tx` pokes the producer (the
/// stdin→child analog); `_thread` is retained so the producer outlives the
/// boot scope (R665 — it actually lives as long as `tx` is held, exiting when
/// the `Owner` drops and the last `Sender` with it).
struct LiveLog {
    lines: Arc<Mutex<Vec<String>>>,
    tx: Sender<()>,
    _thread: std::thread::JoinHandle<()>,
}

/// Self-create (once) the shared buffer + poke channel + producer thread, and
/// return the cached handle. Spawns the thread on first call — therefore
/// invoked from `create_extra_externals` (boot), never the pure `view`.
///
/// `use_repaint_sink()` is resolved *before* the `Owner::cache` factory so the
/// factory never re-enters `Owner::cache` (the R666 nested-factory guard,
/// `owner-cache-no-nested-factory`).
fn use_live_log() -> Rc<LiveLog> {
    let owner = Owner::current().expect("use_live_log() requires an active Owner scope");
    // The public binding-facing hook; resolved here, before the `Owner::cache`
    // factory below, so the factory never re-enters `Owner::cache`.
    let sink = use_repaint_sink();
    // R1269 PR-50 §6.3 — the wake sibling of the repaint sink: the producer
    // thread's data arrival (a pane's `on_dirty` analog) also wakes any parked
    // async `scene/waitFor`, so a wire client repaints on output it did not
    // cause. Resolved before the cache factory, same as `sink`.
    let waiter = use_waiter_registry();
    owner.cache(LOG_KEY, move || {
        let lines: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let (tx, rx) = mpsc::channel::<()>();
        let lines_for_thread = Arc::clone(&lines);
        let thread = std::thread::Builder::new()
            .name("hello-live-data-producer".to_owned())
            .spawn(move || {
                // The producer (PTY-reader analog): block until poked, append a
                // line to the producer-authoritative buffer, then wake the
                // shell to repaint. `recv` errors when every `Sender` has been
                // dropped (the Owner went away) — the thread then exits.
                let mut seq = 0_usize;
                while rx.recv().is_ok() {
                    seq += 1;
                    if let Ok(mut guard) = lines_for_thread.lock() {
                        guard.push(producer_line(seq));
                        let overflow = guard.len().saturating_sub(MAX_LINES);
                        if overflow > 0 {
                            guard.drain(0..overflow);
                        }
                    }
                    sink.request_repaint();
                    // The same dirty edge wakes parked async waitFor waiters
                    // (advances the change generation + fires their replies).
                    waiter.notify_changed();
                }
            })
            .expect("spawn hello-live-data producer thread");
        LiveLog {
            lines,
            tx,
            _thread: thread,
        }
    })
}

/// Snapshot the producer-authoritative buffer for this frame (a poisoned lock
/// degrades to empty rather than panicking the pure view).
fn log_lines(log: &LiveLog) -> Vec<String> {
    log.lines.lock().map(|g| g.clone()).unwrap_or_default()
}

// ─── status / list / button rendering ─────────────────────────────────────

/// The status line — the single SSOT for the visible count text *and* the
/// `role=status` live-region accessible name.
fn status_line(count: usize) -> String {
    match count {
        0 => "No events yet \u{2014} press Tick".to_owned(),
        1 => "1 event".to_owned(),
        n => format!("{n} events"),
    }
}

/// One log row — a tagged container holding the single SSOT line (paint ==
/// `listitem` accessible name).
fn log_row_scene(visible_index: usize, line: &str, theme: &Theme) -> Scene {
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            line.to_owned(),
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
                .with_size(Size::px(WIN_W - 80, 26)),
        ),
    )
}

/// The log panel — the resident lines, or a single placeholder note when empty.
/// `LIST_TAG` stays present across both states.
fn log_list_scene(lines: &[String], theme: &Theme) -> Scene {
    let children: Vec<Scene> = if lines.is_empty() {
        vec![Scene::Container(
            ContainerNode::new(vec![Scene::Text(TextNode::styled(
                "Waiting for background events\u{2026}".to_owned(),
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
                    .with_size(Size::px(WIN_W - 80, 26)),
            ),
        )]
    } else {
        lines
            .iter()
            .enumerate()
            .map(|(i, line)| log_row_scene(i, line, theme))
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

/// Cached posture of the Tick button + its focus flag. The log lines live in
/// the producer-authoritative buffer the owner-scoped view reads directly.
type LiveDataViewState = (ButtonState, bool);

/// view-fn (§6.3): pure sync mapping `(button posture) -> Scene`, reading the
/// producer-authoritative buffer for the status line / rows. No side-effects
/// (the producer thread is spawned in `create_extra_externals`).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(state: LiveDataViewState, _frame: &Frame) -> Scene {
    let (posture, focused) = state;
    let theme = use_theme(THEME_TAG).theme_animated();
    let lines = log_lines(&use_live_log());

    let title = Scene::Text(TextNode::styled(
        "Live event log (off-thread producer)",
        Rect::default(),
        TextStyle::new()
            .with_size_px(16)
            .with_fg(theme.resolve(ColorRole::OnSurface)),
    ));

    let status = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            status_line(lines.len()),
            Rect::default(),
            TextStyle::new()
                .with_size_px(14)
                .with_fg(theme.resolve(ColorRole::OnSurfaceMuted)),
        ))])
        .with_tag(STATUS_TAG)
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_justify(JustifyContent::Center),
        ),
    );

    let list = log_list_scene(&lines, &theme);

    let button = pinion_widget_paint::button::button_scene(
        "Tick",
        posture,
        focused,
        TICK_HOVER_KEY,
        &ButtonColors::filled_tonal(&theme),
        &ButtonStyle::m3_default(TICK_TAG)
            .with_size(Size::px(140, 40))
            .with_label_font_size_px(15),
    );

    Scene::Container(
        ContainerNode::new(vec![title, status, list, button])
            .with_style(BoxStyle::filled(theme.resolve(ColorRole::Surface)))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_align_items(AlignItems::Center)
                    .with_justify(JustifyContent::Center)
                    .with_size(Size::px(WIN_W, WIN_H))
                    .with_gap(16),
            ),
    )
}

struct LiveDataView;

impl WidgetCore for LiveDataView {
    type State = LiveDataViewState;
    type Event = ();

    fn create_external() -> Box<dyn External> {
        Box::new(ButtonExternal::new())
    }

    fn create_extra_externals() -> Vec<ExtraExternal> {
        // Spawn the producer thread FIRST (boot) so the data layer is live
        // before the first paint — and so the side-effecting thread spawn
        // never runs inside the pure view-fn.
        let _log = use_live_log();
        Vec::new()
    }

    fn tag() -> &'static str {
        TICK_TAG
    }

    fn read_state(scene: &Scene) -> LiveDataViewState {
        (
            read_button_state(scene, TICK_TAG),
            read_button_focused(scene, TICK_TAG),
        )
    }

    fn view(state: LiveDataViewState, frame: &Frame) -> Scene {
        view(state, frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    fn title() -> &'static str {
        "pinion hello-live-data (R1000 §5.23)"
    }

    fn keybinding(_key: &str) -> Option<()> {
        None
    }

    fn apply_key(
        scene: &mut Scene,
        focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        apply_aria_activate(scene, focused, key, TICK_TAG)
    }

    /// R1000 reducer: a Tick click only *pokes* the producer thread through the
    /// channel — it never touches the buffer. The producer appends a line and
    /// calls `request_repaint`; the next frame's `view` re-reads the buffer.
    fn update(
        _state: LiveDataViewState,
        intent: &pinion_core::Intent,
    ) -> Vec<pinion_core::command::Command> {
        if intent.tag_str() == "tick.click" {
            // Dropped poke (producer gone) is harmless — the app is tearing down.
            let _ = use_live_log().tx.send(());
        }
        Vec::new()
    }
}

impl WidgetA11y for LiveDataView {
    /// A WAI-ARIA `list` over the resident log lines (one `listitem` each,
    /// named from the same SSOT the paint uses), a `role=status` live region
    /// for the count, and the Tick `button`.
    fn access_node(state: &LiveDataViewState, focused: Option<&str>) -> Vec<AccessNode> {
        let (posture, _focused) = *state;
        let lines = log_lines(&use_live_log());

        let mut list = AccessNode::new(LIST_TAG, AriaRole::List).with_name("Event log");
        let mut items: Vec<AccessNode> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let tag = row_tag(i);
            list = list.with_child(tag.clone());
            items.push(
                AccessNode::new(tag, AriaRole::ListItem)
                    .with_name(line.clone())
                    .with_position_in_set(u32::try_from(i + 1).unwrap_or(u32::MAX))
                    .with_size_of_set(u32::try_from(lines.len()).unwrap_or(u32::MAX)),
            );
        }

        let mut nodes = Vec::with_capacity(items.len() + 3);
        nodes.push(list);
        nodes.extend(items);
        nodes.push(
            AccessNode::new(STATUS_TAG, AriaRole::Status).with_name(status_line(lines.len())),
        );
        nodes.push(
            AccessNode::new(TICK_TAG, AriaRole::Button)
                .with_state(button_a11y_state(posture, focused == Some(TICK_TAG))),
        );
        nodes
    }
}

impl WidgetView for LiveDataView {
    type Renderer = HelloLiveDataRenderer;

    fn initial_size_strategy() -> pinion_shell::SizeStrategy {
        pinion_shell::SizeStrategy::Fixed {
            width: WIN_W,
            height: WIN_H,
        }
    }
}

fn main() {
    pinion_shell::run::<LiveDataView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> LiveDataViewState {
        (ButtonState::Idle, false)
    }

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

    fn intent(tag: &str) -> pinion_core::Intent {
        pinion_core::Intent::new_owned(tag.to_owned(), pinion_core::external::IntrospectValue::Null)
    }

    #[test]
    fn producer_line_is_single_ssot() {
        assert_eq!(producer_line(1), "[001] background event #1");
        assert_eq!(producer_line(42), "[042] background event #42");
    }

    #[test]
    fn status_line_singular_and_plural() {
        assert!(status_line(0).contains("Tick"));
        assert_eq!(status_line(1), "1 event");
        assert_eq!(status_line(3), "3 events");
    }

    #[test]
    fn empty_log_shows_placeholder() {
        let owner = Owner::new();
        let scene = owner.run(|| view(idle(), &Frame::new()));
        assert_eq!(
            text_under(&scene, STATUS_TAG).as_deref(),
            Some("No events yet \u{2014} press Tick"),
        );
        assert!(
            text_under(&scene, &row_tag(0))
                .as_deref()
                .is_some_and(|t| t.contains("Waiting")),
            "empty log shows the placeholder note",
        );
    }

    #[test]
    fn view_renders_buffered_lines() {
        // Push to the producer-authoritative buffer directly (no thread needed)
        // → deterministic: the view reads whatever the producer wrote.
        let owner = Owner::new();
        let scene = owner.run(|| {
            let log = use_live_log();
            {
                let mut g = log.lines.lock().unwrap();
                g.push(producer_line(1));
                g.push(producer_line(2));
            }
            view(idle(), &Frame::new())
        });
        assert_eq!(text_under(&scene, STATUS_TAG).as_deref(), Some("2 events"));
        assert_eq!(
            text_under(&scene, &row_tag(0)).as_deref(),
            Some("[001] background event #1"),
        );
        assert_eq!(
            text_under(&scene, &row_tag(1)).as_deref(),
            Some("[002] background event #2"),
        );
    }

    #[test]
    fn buffer_windows_to_max_lines() {
        let owner = Owner::new();
        owner.run(|| {
            let log = use_live_log();
            let mut g = log.lines.lock().unwrap();
            for n in 1..=(MAX_LINES + 4) {
                g.push(producer_line(n));
                let overflow = g.len().saturating_sub(MAX_LINES);
                if overflow > 0 {
                    g.drain(0..overflow);
                }
            }
            assert_eq!(g.len(), MAX_LINES, "buffer windows to MAX_LINES");
        });
    }

    #[test]
    fn tick_pokes_producer_thread_which_appends_and_would_repaint() {
        // The motivating end-to-end path: a Tick click pokes the producer
        // channel; the background thread appends to the buffer and calls
        // `request_repaint` (a NullRepaintSink no-op off the live event loop).
        // Cross-thread, so a bounded poll (no wall-clock assertion) confirms
        // the append — the executor-test pattern; the thread responds in µs.
        let owner = Owner::new();
        owner.run(|| {
            let log = use_live_log();
            assert!(log.lines.lock().unwrap().is_empty());
            let _ = LiveDataView::update(idle(), &intent("tick.click"));
            let mut appended = false;
            for _ in 0..200 {
                if log.lines.lock().unwrap().len() == 1 {
                    appended = true;
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            assert!(
                appended,
                "producer thread appended a line after the tick poke"
            );
            assert_eq!(log.lines.lock().unwrap()[0], "[001] background event #1",);
        });
    }

    #[test]
    fn a11y_list_status_and_button() {
        let owner = Owner::new();
        let nodes = owner.run(|| {
            let log = use_live_log();
            log.lines.lock().unwrap().push(producer_line(1));
            <LiveDataView as WidgetA11y>::access_node(&idle(), None)
        });
        let list = nodes.iter().find(|n| n.tag == LIST_TAG).expect("list node");
        assert_eq!(list.role, AriaRole::List);
        assert_eq!(list.children.len(), 1, "one resident line claimed");
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.role == AriaRole::ListItem)
                .count(),
            1,
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == STATUS_TAG && n.role == AriaRole::Status)
        );
        assert!(
            nodes
                .iter()
                .any(|n| n.tag == TICK_TAG && n.role == AriaRole::Button)
        );
    }

    #[test]
    fn tick_button_is_focusable() {
        // §5.39: collected from the paint scene.
        let scene = Owner::new().run(|| view(idle(), &Frame::new()));
        assert_eq!(scene.collect_focusable_tags(), vec![TICK_TAG.to_owned()]);
    }
}
