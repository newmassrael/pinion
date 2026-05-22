pub mod animation;
pub mod app;
pub mod clipboard;
pub mod command;
pub mod event;
pub mod external;
pub mod frame;
pub mod input;
pub mod intent;
pub mod reactive;
pub mod renderer;
pub mod revision;
pub mod scene;
pub mod style;
pub mod theme;
pub mod topology;
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
    Tickable, Tween,
};
pub use clipboard::{Clipboard, ClipboardSelection, InMemoryClipboard};
pub use command::Command;
pub use event::Event;
pub use renderer::WidgetRenderer;
pub use external::External;
pub use widget_core::WidgetCore;
pub use frame::Frame;
pub use input::{CompositionEvent, Modifiers};
pub use intent::{Intent, IntentTag};
pub use reactive::{
    Computed, Effect, FetchToken, IntoIntrospectValue, JsonValue, LocalSpawner, Owner,
    OwnerSnapshot, Resource, ResourceState, Signal, SignalExternal, SnapshotRestoreError,
    SnapshotableSignal, batch,
};
pub use revision::SceneRevision;
pub use scene::{HitPath, Scene};
pub use style::{
    Align, AlignItems, Border, BoxStyle, Color, Display, Fit, FlexDirection, FontStyle,
    FontWeight, ImageStyle, JustifyContent, LayoutStyle, LineHeight, PathStyle, Size, SizeValue,
    Stroke, StrokeCap, TextAlign, TextDecoration, TextOverflow, TextStyle,
    scale_normalized_to_px,
};
pub use theme::{ColorRole, Theme, ThemeProvider, use_theme};
