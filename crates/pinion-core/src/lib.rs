// R822 — `clippy::large_stack_arrays` fires, **only on the unit-test target**,
// on a 16 KB+ array that is not pinion-core code: libtest's generated runner
// builds a `[&test::TestDescAndFn; N]` descriptor table — one 8-byte reference
// per `#[test]` in the crate. At N = 2048 that table is exactly 16384 bytes (the
// lint threshold; it fires on strictly `>`), so the 2049th test tips it over and
// the lint blames the synthetic runner, whose span collapses to the crate root
// (`lib.rs:1`) where no item-scoped `#[allow]` can reach. (The earlier diagnosis
// — a serde `Signal<Vec<_>>` deserialization buffer, R820/R821 — was wrong:
// MIR shows the only >16384-byte array in the test target is
// `[&test::TestDescAndFn; N]`, and N equals the crate's `#[test]` count exactly.)
//
// Gating the allow on `cfg(test)` relaxes it for the test target only; the
// production `--lib` lint pass (and every downstream crate) still denies
// `large_stack_arrays` at full strength, so a genuine oversized stack array in
// pinion-core's non-test code is still caught. This restores headroom to add
// in-crate unit tests for core logic rather than exiling them to examples.
#![cfg_attr(test, allow(clippy::large_stack_arrays))]

pub mod animation;
pub mod app;
pub mod cell_metric;
pub mod cell_value;
pub mod clipboard;
pub mod command;
pub mod composite_tag;
pub mod event;
pub mod external;
pub mod file_dialog;
pub mod focus_request;
pub mod frame;
pub mod input;
pub mod intent;
pub mod modal_scope_request;
pub mod directory;
pub mod print;
pub mod reactive;
pub mod renderer;
pub mod revision;
pub mod scene;
pub mod storage;
pub mod style;
pub mod syntax;
pub mod term_grid;
pub mod text_scale;
pub mod theme;
pub mod topology;
pub mod tray;
pub mod undo;
pub mod widget_core;
pub mod widgets;

// R51.127 §5.41 — substrate-level test fixtures shared across
// `pinion-runtime` + `pinion-tui` test suites. Gated behind the
// `test-fixtures` feature so production binaries never see them.
#[cfg(any(test, feature = "test-fixtures"))]
pub mod test_fixtures;

#[cfg(test)]
mod multi_window;

pub use animation::{
    Animatable, AnimRect, AnimVec2, AnimVec4, Animation, Easing, SpringConfig, SpringState,
    Tickable, Tween, DEFAULT_REST_EPSILON,
};
pub use cell_metric::CellMetric;
pub use cell_value::{CellKind, CellValue};
pub use clipboard::{Clipboard, ClipboardSelection, InMemoryClipboard};
pub use command::Command;
pub use event::Event;
pub use file_dialog::{
    DialogKind, FileDialog, FileDialogFuture, FileDialogRequest, FileFilter, ScriptedCall,
    ScriptedFileDialog,
};
pub use renderer::WidgetRenderer;
pub use external::External;
pub use widget_core::{WidgetCore, WidgetEventName, WidgetStateName, WidgetTag};
pub use frame::Frame;
pub use input::{
    edit_field_keymap, forward_key_to_field, CompositionEvent, DragCalibration, DragLatch, HeldKeys,
    InputStateSnapshot, Modifiers, SelectionChord, DRAG_CLICK_THRESHOLD_PX,
};
pub use intent::{Intent, IntentTag};
pub use reactive::{
    is_simulating, use_local_task_pump, use_repaint_sink, Computed, DeferredReady, Effect,
    FetchToken, IntoIntrospectValue, JsonValue, LocalSpawner, LocalTaskPump, NullRepaintSink,
    Owner, OwnerSnapshot, RepaintSink, Resource, ResourceCache, ResourceState, Signal,
    SignalExternal, SimulationGuard, SnapshotRestoreError, SnapshotableSignal, batch,
};
pub use revision::SceneRevision;
pub use scene::{HitPath, Scene};
pub use directory::{DirEntry, Directory, InMemoryDirectory};
pub use storage::{InMemoryStorage, Storage};
pub use style::{
    Align, AlignItems, Border, BoxStyle, Color, ColorStop, Display, Extend, Fit, FlexDirection,
    FontFamily, FontStyle, FontWeight, GenericFontFamily, Gradient, GradientKind, ImageStyle,
    JustifyContent, LayoutStyle, LineHeight, PathStyle, Size, SizeValue, Stroke, StrokeCap,
    TextAlign, TextDecoration, TextOverflow, TextStyle, scale_normalized_to_px,
};
pub use syntax::{highlight_code, SyntaxPalette};
pub use term_grid::{
    CellAttrs, CellWidth, ColorTarget, CursorShape, GridBuffer, GridCursor, Palette, ScreenKind,
    TermCell, TermColor,
};
pub use theme::{
    ColorRole, SystemColorScheme, THEME_FADE_SPRING, Theme, ThemeMode, ThemeProvider,
    set_system_color_scheme, system_color_scheme, use_theme,
};
