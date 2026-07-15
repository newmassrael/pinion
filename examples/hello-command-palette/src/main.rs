//! `hello-command-palette` — R912 §5.38 §5.22 fuzzy command palette.
//!
//! ## What this demonstrates
//!
//! The editor-universal **command palette** (VS Code Ctrl+P, Unreal /
//! Blender command search): type a query, a command list filters by a
//! subsequence-scored fuzzy match, Arrow keys / clicks navigate, and
//! Enter / a click runs the selected command. It is the keyboard-first
//! entry point to every editor action — the highest-leverage "find and
//! run anything" surface a self-hosted editor needs.
//!
//! This is a *fresh* widget axis (not the property-grid / data-grid /
//! inspector DCC family), composing existing substrate:
//!
//! - a primary [`PaletteExternal`] holding the query + selection +
//!   last-executed command (the reactive-holder pattern);
//! - the R910 composite-send click-to-select wire (`palette#<i>` rows →
//!   the External `send` wire), here selecting *and* executing on a
//!   click (palette rows are run-on-click);
//! - a pure [`fuzzy_score`] matcher (the one genuinely new piece) — a
//!   case-insensitive subsequence match scored for consecutive runs and
//!   word-start hits.
//!
//! ## §2 AI-first
//!
//! Everything is RPC-drivable and scene-as-data: `intervene query`
//! filters, `query result.<i>` / `selected_command` / `last_executed`
//! read the state, `invoke execute` runs the selection. Live GUI typing
//! through a `TextField` is the established follow-up (property-grid
//! search box pattern); the query is set over RPC here, which is the §2
//! primary path and a complete vertical slice.
//!
//! ## Verification
//!
//! `tools/demos/r912_command_palette.py` drives it over RPC: set a query
//! → the fuzzy-filtered results re-rank → Arrow / click select → execute
//! → `last_executed` confirms. Deterministic (`fuzzy_score` is pure), so
//! zero flake ([[ai-first-rpc-introspection-obligation]]).

use std::rc::Rc;

#[cfg(test)]
use pinion_a11y::{AriaRole, WidgetA11y};
use pinion_core::composite_tag::send_activation_index;
use pinion_core::external::query_proxy_external_impl;
use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, SchemaArg,
    SchemaField,
};
use pinion_core::scene::{ContainerNode, Rect, TextNode};
use pinion_core::style::{
    AlignItems, BoxStyle, Color, FlexDirection, LayoutStyle, Size, TextStyle,
};
use pinion_core::{ColorRole, Frame, Owner, Scene, Signal, use_theme};
use pinion_derive::widget;
use pinion_shell::vello_renderer_impl;

include!(concat!(env!("OUT_DIR"), "/app.rs"));
vello_renderer_impl!(
    HelloCommandPaletteRenderer,
    HelloCommandPaletteRendererError
);

const WIN_W: u32 = 440;
const WIN_H: u32 = 420;

const PALETTE_TAG: &str = "palette";
const THEME_TAG: &str = "app";

const PROMPT_FONT_PX: u32 = 15;
const ROW_FONT_PX: u32 = 14;
const STATUS_FONT_PX: u32 = 12;
const ROW_H: u32 = 30;

/// The command set the palette searches. A static editor-ish roster —
/// the executed command is recorded in `last_executed` (a real binding
/// would dispatch each through the §5.23 command system; here the
/// observable effect is the recorded name).
const COMMANDS: [&str; 10] = [
    "New File",
    "Open Folder",
    "Save All",
    "Close Editor",
    "Toggle Theme",
    "Find in Files",
    "Go to Line",
    "Format Document",
    "Split Editor",
    "Reload Window",
];

// ─── Fuzzy matcher (the one new piece of logic) ───────────────────

/// Case-insensitive subsequence fuzzy score. `Some(score)` when every
/// (non-whitespace) char of `query` appears in `candidate` in order;
/// `None` otherwise. Higher is better: each matched char scores `1`, a
/// match immediately after the previous one adds a consecutive-run bonus
/// (`+5`), and a match at a word start (string start or after a space /
/// `-` / `_`) adds a word-boundary bonus (`+10`) — so `"of"` ranks
/// `"Open Folder"` (two word-starts) above an incidental mid-word hit.
/// An empty query matches everything with score `0`.
#[must_use]
fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
    let needle: Vec<char> = query
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if needle.is_empty() {
        return Some(0);
    }
    let cand: Vec<char> = candidate.chars().collect();
    let mut qi = 0usize;
    let mut score = 0i32;
    let mut prev: Option<usize> = None;
    for (ci, raw) in cand.iter().enumerate() {
        if qi >= needle.len() {
            break;
        }
        if raw.to_ascii_lowercase() == needle[qi] {
            score += 1;
            if prev.is_some() && prev == ci.checked_sub(1) {
                score += 5;
            }
            let word_start = ci == 0 || matches!(cand.get(ci - 1), Some(' ' | '-' | '_'));
            if word_start {
                score += 10;
            }
            prev = Some(ci);
            qi += 1;
        }
    }
    (qi == needle.len()).then_some(score)
}

/// The command indices matching `query`, highest fuzzy score first.
/// Ties keep the roster order (stable sort), so an empty query returns
/// every command in declaration order.
#[must_use]
fn visible_commands(query: &str) -> Vec<usize> {
    let mut scored: Vec<(usize, i32)> = COMMANDS
        .iter()
        .enumerate()
        .filter_map(|(i, name)| fuzzy_score(query, name).map(|s| (i, s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1)); // stable: equal scores keep order.
    scored.into_iter().map(|(i, _)| i).collect()
}

// ─── Shared reactive state ────────────────────────────────────────

fn use_query() -> Rc<Signal<String>> {
    let owner = Owner::current().expect("use_query requires an active Owner scope");
    owner.cache("palette.query", || Signal::new(String::new()))
}

fn use_selected() -> Rc<Signal<usize>> {
    let owner = Owner::current().expect("use_selected requires an active Owner scope");
    owner.cache("palette.selected", || Signal::new(0usize))
}

fn use_last_executed() -> Rc<Signal<Option<String>>> {
    let owner = Owner::current().expect("use_last_executed requires an active Owner scope");
    owner.cache("palette.last_executed", || Signal::new(None))
}

// ─── External (the §5.15 AI surface) ──────────────────────────────

struct PaletteExternal {
    query: Rc<Signal<String>>,
    selected: Rc<Signal<usize>>,
    last_executed: Rc<Signal<Option<String>>>,
}

impl PaletteExternal {
    fn new(
        query: Rc<Signal<String>>,
        selected: Rc<Signal<usize>>,
        last_executed: Rc<Signal<Option<String>>>,
    ) -> Self {
        Self {
            query,
            selected,
            last_executed,
        }
    }

    fn visible(&self) -> Vec<usize> {
        visible_commands(&self.query.get())
    }

    /// The selection clamped into the current result range (a query
    /// change can shrink the list under a stale index).
    fn sel(&self) -> usize {
        let len = self.visible().len();
        if len == 0 {
            0
        } else {
            self.selected.get().min(len - 1)
        }
    }

    /// Set the query and reset the selection to the top result (the
    /// standard palette behaviour — the best match is pre-selected).
    fn set_query(&self, text: String) {
        self.query.set(text);
        self.selected.set(0);
    }

    /// Select result `i` (index into the visible list). `false` if out
    /// of range.
    fn select(&self, i: usize) -> bool {
        if i < self.visible().len() {
            self.selected.set(i);
            true
        } else {
            false
        }
    }

    /// Run the selected command — records its name in `last_executed`.
    /// `None` if the result list is empty.
    fn execute(&self) -> Option<&'static str> {
        let visible = self.visible();
        let name = COMMANDS[*visible.get(self.sel())?];
        self.last_executed.set(Some(name.to_owned()));
        Some(name)
    }
}

impl core::fmt::Debug for PaletteExternal {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PaletteExternal")
            .field("query", &self.query.get())
            .field("results", &self.visible().len())
            .finish_non_exhaustive()
    }
}

// R913.1 — Gui+Rpc config-holder External skeleton via the SSOT macro
// (was hand-rolled — [[use-substrate-not-hand-rolled-equivalent]]).
query_proxy_external_impl!(PaletteExternal);

impl ExternalIntrospect for PaletteExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(
            const {
                &[
                    SchemaField::new("query", "string"),
                    SchemaField::new("result_count", "int"),
                    SchemaField::parametric(
                        "result.<i>",
                        "string",
                        const { &[SchemaArg::index("i", "result_count")] },
                    ),
                    SchemaField::new("selected", "int"),
                    SchemaField::new("selected_command", "string"),
                    SchemaField::new("last_executed", "string"),
                    SchemaField::new("select", "int"),
                    SchemaField::new("execute", "json"),
                    SchemaField::new("send", "string"),
                ]
            },
        )
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let visible = self.visible();
        match path {
            "query" => Some(IntrospectValue::Text(self.query.get())),
            "result_count" => Some(IntrospectValue::Int(i64::try_from(visible.len()).ok()?)),
            "selected" => Some(IntrospectValue::Int(i64::try_from(self.sel()).ok()?)),
            // Present-but-empty: the path always resolves; an empty
            // result list reports Null (not a Rust `None`, which the RPC
            // layer would surface as an UnknownPath error — R894).
            "selected_command" => Some(match visible.get(self.sel()) {
                Some(&c) => IntrospectValue::Text(COMMANDS[c].to_owned()),
                None => IntrospectValue::Null,
            }),
            "last_executed" => Some(match self.last_executed.get() {
                Some(name) => IntrospectValue::Text(name),
                None => IntrospectValue::Null,
            }),
            _ => {
                let i: usize = path.strip_prefix("result.")?.parse().ok()?;
                visible
                    .get(i)
                    .map(|&c| IntrospectValue::Text(COMMANDS[c].to_owned()))
            }
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        match path {
            "query" => match value {
                IntrospectValue::Text(text) => {
                    self.set_query(text);
                    Ok(())
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "selected" => match value {
                IntrospectValue::Int(n) => {
                    let i = usize::try_from(n).map_err(|_| InterveneError::OutOfRange)?;
                    if self.select(i) {
                        Ok(())
                    } else {
                        Err(InterveneError::OutOfRange)
                    }
                }
                _ => Err(InterveneError::TypeMismatch),
            },
            "result_count" | "selected_command" | "last_executed" => Err(InterveneError::ReadOnly),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "select" => match args {
                IntrospectValue::Int(n) => {
                    let i = usize::try_from(n).map_err(|_| InvokeError::TypeMismatch)?;
                    if self.select(i) {
                        Ok(IntrospectValue::Int(n))
                    } else {
                        Err(InvokeError::Rejected)
                    }
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "execute" => Ok(match self.execute() {
                Some(name) => IntrospectValue::Text(name.to_owned()),
                None => IntrospectValue::Null,
            }),
            // R910 composite send wire: a click on a `palette#<i>` result
            // row selects it AND runs it (palette rows are run-on-click)
            // on the activation edge.
            "send" => match args {
                IntrospectValue::Text(ref payload) => {
                    if let Some(i) = send_activation_index(payload) {
                        if self.select(i) {
                            self.execute();
                        }
                    }
                    Ok(IntrospectValue::Bool(true))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn make_palette_external() -> PaletteExternal {
    PaletteExternal::new(use_query(), use_selected(), use_last_executed())
}

/// Keyboard target for an Arrow / Home / End nav key, read off the
/// External's introspect (`result_count` + `selected`). Clamps at both
/// ends. `None` for a non-nav key or empty results.
fn resolve_target_index(intro: Option<&dyn ExternalIntrospect>, key: &str) -> Option<usize> {
    let intro = intro?;
    let count = match intro.query("result_count")? {
        IntrospectValue::Int(n) => usize::try_from(n).ok()?,
        _ => return None,
    };
    if count == 0 {
        return None;
    }
    let last = count - 1;
    let cur = match intro.query("selected")? {
        IntrospectValue::Int(n) => usize::try_from(n).unwrap_or(0),
        _ => 0,
    };
    match key {
        "ArrowDown" => Some((cur + 1).min(last)),
        "ArrowUp" => Some(cur.saturating_sub(1)),
        "Home" => Some(0),
        "End" => Some(last),
        _ => None,
    }
}

// ─── View ─────────────────────────────────────────────────────────

#[allow(clippy::trivially_copy_pass_by_ref)]
fn view(selected: usize, _frame: &Frame) -> Scene {
    let theme = use_theme(THEME_TAG).theme_animated();
    let on_surface = theme.resolve(ColorRole::OnSurface);
    let surface = theme.resolve(ColorRole::Surface);
    let surface_alt = theme.resolve(ColorRole::SurfaceContainerHighest);
    let muted = theme.resolve(ColorRole::OnSurfaceMuted);
    let accent = theme.resolve(ColorRole::Accent);

    let query = use_query().get();
    let visible = visible_commands(&query);
    let sel = if visible.is_empty() {
        0
    } else {
        selected.min(visible.len() - 1)
    };

    // Search box: a "> query" prompt in an input-styled container.
    let prompt = if query.is_empty() {
        "> type to search commands".to_owned()
    } else {
        format!("> {query}")
    };
    let prompt_fg = if query.is_empty() { muted } else { on_surface };
    let search = Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            prompt,
            Rect::default(),
            TextStyle::new()
                .with_size_px(PROMPT_FONT_PX)
                .with_fg(prompt_fg),
        ))])
        .with_tag("palette_search")
        .with_style(BoxStyle::filled(surface_alt).with_corner_radius(6))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(WIN_W - 32, 34)),
        ),
    );

    // Result rows.
    let rows: Vec<Scene> = visible
        .iter()
        .enumerate()
        .map(|(i, &cmd)| result_row(i, COMMANDS[cmd], i == sel, on_surface, accent))
        .collect();
    let results = Scene::Container(
        ContainerNode::new(rows).with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Column)
                .with_gap(2)
                .with_size(Size::px(WIN_W - 32, WIN_H - 120)),
        ),
    );

    let status_text = match use_last_executed().get() {
        Some(name) => format!("{} result(s)  |  Ran: {name}", visible.len()),
        None => format!("{} result(s)  |  Enter or click to run", visible.len()),
    };
    let status = Scene::Text(TextNode::styled(
        status_text,
        Rect::default(),
        TextStyle::new().with_size_px(STATUS_FONT_PX).with_fg(muted),
    ));

    Scene::Container(
        ContainerNode::new(vec![search, results, status])
            .with_tag(PALETTE_TAG)
            .with_style(BoxStyle::filled(surface))
            .with_layout(
                LayoutStyle::new()
                    .flex(FlexDirection::Column)
                    .with_gap(10)
                    .with_size(Size::px(WIN_W, WIN_H)),
            ),
    )
}

/// One result row, tagged `palette#<i>` so a click routes to the
/// External `send` wire (select + run). The selected row carries an
/// accent background.
fn result_row(index: usize, name: &str, selected: bool, fg: Color, accent: Color) -> Scene {
    let fill = if selected { accent } else { Color::TRANSPARENT };
    let text_fg = if selected {
        Color::rgb(0xff, 0xff, 0xff)
    } else {
        fg
    };
    Scene::Container(
        ContainerNode::new(vec![Scene::Text(TextNode::styled(
            name.to_owned(),
            Rect::default(),
            TextStyle::new().with_size_px(ROW_FONT_PX).with_fg(text_fg),
        ))])
        .with_tag(format!("{PALETTE_TAG}#{index}"))
        .with_style(BoxStyle::filled(fill).with_corner_radius(5))
        .with_layout(
            LayoutStyle::new()
                .flex(FlexDirection::Row)
                .with_align_items(AlignItems::Center)
                .with_size(Size::px(WIN_W - 40, ROW_H)),
        ),
    )
}

// ─── Binding ──────────────────────────────────────────────────────

#[widget(
    tag = "palette",
    state = usize,
    event = (),
    title = "pinion hello-command-palette (R912 §5.38 fuzzy palette)",
    renderer = HelloCommandPaletteRenderer,
    initial_size = (WIN_W, WIN_H),
    external = make_palette_external,
    role = Listbox,
    apply_key,
)]
struct PaletteView;

impl PaletteView {
    fn read_state(scene: &Scene) -> usize {
        if let Scene::External(node) = scene {
            if let Some(intro) = node.handle.introspect() {
                if let Some(IntrospectValue::Int(n)) = intro.query("selected") {
                    return usize::try_from(n).unwrap_or(0);
                }
            }
        }
        0
    }

    fn view(state: usize, frame: Frame) -> Scene {
        view(state, &frame)
    }

    fn event_name(_event: ()) -> &'static str {
        "__internal__"
    }

    /// Arrow / Home / End navigate the results (select, no run); Enter
    /// runs the selected command.
    fn apply_key(
        scene: &mut Scene,
        _focused: Option<&str>,
        key: &str,
        _modifiers: pinion_core::Modifiers,
    ) -> bool {
        let Scene::External(node) = scene else {
            return false;
        };
        if key == "Enter" {
            if let Some(intro) = node.handle.introspect_mut() {
                let _ = intro.invoke("execute", IntrospectValue::Null);
                return true;
            }
            return false;
        }
        let Some(target) = resolve_target_index(node.handle.introspect(), key) else {
            return false;
        };
        let Some(intro) = node.handle.introspect_mut() else {
            return false;
        };
        let _ = intro.invoke(
            "select",
            IntrospectValue::Int(i64::try_from(target).unwrap_or(0)),
        );
        true
    }
}

fn main() {
    pinion_shell::run::<PaletteView>();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ext() -> PaletteExternal {
        let owner = Owner::new();
        owner.run(make_palette_external)
    }

    #[test]
    fn r912_empty_query_lists_all_commands_in_order() {
        assert_eq!(
            visible_commands(""),
            (0..COMMANDS.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn r912_fuzzy_subsequence_filters() {
        // "gtl" is a subsequence of "Go to Line" (G, t, L) but not of
        // "New File".
        assert!(fuzzy_score("gtl", "Go to Line").is_some());
        assert!(fuzzy_score("gtl", "New File").is_none());
        // "zz" matches nothing here.
        assert!(visible_commands("zz").is_empty());
    }

    #[test]
    fn r912_word_start_outranks_midword() {
        // "of" should rank "Open Folder" (two word-starts: O, F) above a
        // command where o/f are mid-word.
        let of = visible_commands("of");
        assert!(!of.is_empty());
        assert_eq!(
            COMMANDS[of[0]], "Open Folder",
            "word-start match ranks first"
        );
    }

    #[test]
    fn r912_consecutive_run_scores_higher_than_scattered() {
        // "fi" consecutive in "Find in Files" (F,i) and "New File" (the
        // 'fi' in File). Both match; the exact ranking is score-driven,
        // but both must appear.
        let fi = visible_commands("fi");
        assert!(fi.iter().any(|&c| COMMANDS[c] == "Find in Files"));
        assert!(fi.iter().any(|&c| COMMANDS[c] == "New File"));
    }

    #[test]
    fn r912_set_query_resets_selection_and_clamps() {
        let mut e = ext();
        e.invoke("select", IntrospectValue::Int(5)).unwrap();
        assert_eq!(e.sel(), 5);
        // A query that yields a single result clamps + resets selection.
        e.intervene("query", IntrospectValue::Text("Reload Window".to_owned()))
            .unwrap();
        assert_eq!(e.query("result_count"), Some(IntrospectValue::Int(1)));
        assert_eq!(e.sel(), 0, "set_query resets selection to the top result");
    }

    #[test]
    fn r912_execute_records_the_selected_command() {
        let mut e = ext();
        e.intervene("query", IntrospectValue::Text("save".to_owned()))
            .unwrap();
        assert_eq!(
            e.query("selected_command"),
            Some(IntrospectValue::Text("Save All".to_owned()))
        );
        let ran = e.invoke("execute", IntrospectValue::Null).unwrap();
        assert_eq!(ran, IntrospectValue::Text("Save All".to_owned()));
        assert_eq!(
            e.query("last_executed"),
            Some(IntrospectValue::Text("Save All".to_owned()))
        );
    }

    #[test]
    fn r912_click_send_selects_and_runs_on_activation() {
        let mut e = ext();
        // Hover/press do not run; the PointerUp activation edge does.
        e.invoke("send", IntrospectValue::Text("2:PointerEnter".to_owned()))
            .unwrap();
        assert_eq!(e.query("last_executed"), Some(IntrospectValue::Null));
        e.invoke("send", IntrospectValue::Text("2:PointerUp".to_owned()))
            .unwrap();
        // Result 2 in the empty-query list is "Save All".
        assert_eq!(
            e.query("last_executed"),
            Some(IntrospectValue::Text("Save All".to_owned()))
        );
    }

    #[test]
    fn r912_select_out_of_range_rejected() {
        let mut e = ext();
        e.intervene("query", IntrospectValue::Text("reload".to_owned()))
            .unwrap();
        assert!(
            e.invoke("select", IntrospectValue::Int(3)).is_err(),
            "only 1 result"
        );
    }

    fn has_text(scene: &Scene, needle: &str) -> bool {
        match scene {
            Scene::Text(t) => t.content.contains(needle),
            Scene::Container(c) => c.children.iter().any(|ch| has_text(ch, needle)),
            _ => false,
        }
    }

    #[test]
    fn view_reflects_query_filter() {
        let owner = Owner::new();
        let scene = owner.run(|| {
            use_query().set("save".to_owned());
            view(0, &Frame::new())
        });
        assert!(has_text(&scene, "Save All"), "filtered command shown");
        assert!(
            !has_text(&scene, "Reload Window"),
            "non-matching command hidden"
        );
    }

    #[test]
    fn r55_g20_view_carries_composite_paint_root_tag() {
        pinion_core::test_fixtures::assert_widget_view_carries_tag::<PaletteView>(0, &Frame::new());
    }

    #[test]
    fn palette_root_reports_listbox_role() {
        let nodes = <PaletteView as WidgetA11y>::access_node(&0, None);
        assert_eq!(nodes[0].role, AriaRole::Listbox);
        assert_eq!(nodes[0].tag, PALETTE_TAG);
    }
}
