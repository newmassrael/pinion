pub mod animation;
pub mod app;
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
pub mod reactive;
pub mod renderer;
pub mod revision;
pub mod scene;
pub mod storage;
pub mod style;
pub mod text_scale;
pub mod theme;
pub mod topology;
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
pub use input::{CompositionEvent, Modifiers};
pub use intent::{Intent, IntentTag};
pub use reactive::{
    is_simulating, Computed, Effect, FetchToken, IntoIntrospectValue, JsonValue, LocalSpawner,
    Owner, OwnerSnapshot, Resource, ResourceState, Signal, SignalExternal,
    SimulationGuard, SnapshotRestoreError, SnapshotableSignal, batch,
};
pub use revision::SceneRevision;
pub use scene::{HitPath, Scene};
pub use storage::{InMemoryStorage, Storage};
pub use style::{
    Align, AlignItems, Border, BoxStyle, Color, ColorStop, Display, Extend, Fit, FlexDirection,
    FontStyle, FontWeight, Gradient, GradientKind, ImageStyle, JustifyContent, LayoutStyle,
    LineHeight, PathStyle, Size, SizeValue, Stroke, StrokeCap, TextAlign, TextDecoration,
    TextOverflow, TextStyle, scale_normalized_to_px,
};
pub use theme::{
    ColorRole, SystemColorScheme, THEME_FADE_SPRING, Theme, ThemeMode, ThemeProvider,
    set_system_color_scheme, system_color_scheme, use_theme,
};
