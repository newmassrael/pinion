//! Fine-grained reactivity primitives per §5.22.
//!
//! Signal/Computed/Resource modeled after Solid.js/Vue3/SwiftUI lineage.
//! All Signals carry `Serialize + DeserializeOwned` per §5.31 (R36) so the
//! hot-reload snapshot protocol can round-trip values across code swaps.

pub mod computed;
pub mod introspect;
pub mod owner;
pub mod resource;
pub mod signal;

pub use computed::Computed;
pub use introspect::{IntoIntrospectValue, JsonValue, SignalExternal};
pub use owner::{Owner, OwnerSnapshot, SnapshotRestoreError, SnapshotableSignal, batch};
pub use resource::{FetchToken, LocalSpawner, Resource, ResourceState};
pub use signal::Signal;
