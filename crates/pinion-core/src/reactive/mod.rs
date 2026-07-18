//! Fine-grained reactivity primitives per §5.22.
//!
//! Signal/Computed/Resource modeled after Solid.js/Vue3/SwiftUI lineage.
//! All Signals carry `Serialize + DeserializeOwned` per §5.31 (R36) so the
//! hot-reload snapshot protocol can round-trip values across code swaps.

pub mod computed;
pub mod effect;
pub mod font_metrics;
pub mod introspect;
pub mod owner;
pub mod pane_viewport;
pub mod provider_slot;
pub mod quit;
pub mod repaint;
pub mod resource;
pub mod resource_cache;
pub mod signal;
pub mod simulation;
pub mod viewport;

pub use computed::Computed;
pub use effect::Effect;
pub use font_metrics::{
    MONOSPACE_METRICS, MonospaceMetrics, NullMonospaceMetrics, measured_monospace_cell,
};
pub use introspect::{IntoIntrospectValue, JsonValue, SignalExternal};
pub use owner::{Owner, OwnerSnapshot, SnapshotRestoreError, SnapshotableSignal, batch};
pub use pane_viewport::use_pane_viewport_size;
pub use provider_slot::{ProviderSlot, SLOT_KEY_PREFIX, SlotScope, is_slot_key};
pub use quit::{NullQuitSink, QUIT_SINK, QuitSink, use_quit_sink};
pub use repaint::{NullRepaintSink, REPAINT_SINK, RepaintSink, use_repaint_sink};
pub use resource::{
    DeferredReady, FetchToken, LOCAL_TASK_PUMP, LocalSpawner, LocalTaskPump, Resource,
    ResourceState, use_local_task_pump,
};
pub use resource_cache::ResourceCache;
pub use signal::Signal;
pub use simulation::{SimulationGuard, is_simulating};
pub use viewport::{VIEWPORT_SIZE, use_viewport_size};
