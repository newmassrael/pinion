//! RPC introspection bridge for reactive primitives (§5.22 R26).
//!
//! Caveat lock: "RPC introspect = `Signal<T:Serialize>` via scene/query;
//! rewind sets via deserialize". This module wires that contract onto the
//! existing §5.15 `ExternalIntrospect` surface so `scene/query` /
//! `scene/rewind` work today without bespoke RPC plumbing.
//!
//! Scope: scalar T whose value already round-trips through `IntrospectValue`
//! (i32 / i64 / f64 / bool / String). Structured T (struct/Vec) carries
//! forward — it requires extending `IntrospectValue` with a `Json` variant,
//! which §5.15 documented as deferred.
//!
//! Shape: `SignalExternal<T>` wraps a `Signal<T>` plus an `IntoIntrospectValue`
//! impl. It implements `External` (so it can sit in the `Scene` tree) and
//! `ExternalIntrospect` (so the existing RPC dispatch routes through it).
//! The wrapped path is always `"value"`; multi-slot signal groupings will
//! emerge when Forge codegen lands (R38).

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::event::Event;
use crate::external::{
    Backend, BackendFallback, BackendSupport, External, ExternalIntrospect, InterveneError,
    IntrospectSchema, IntrospectValue, RepaintOwner, ThreadOwnership,
};

use super::signal::Signal;

/// Mapping between a scalar reactive payload `T` and the
/// `IntrospectValue` wire variant. Implemented for the scalar types
/// the §5.15 wire format already carries.
pub trait IntoIntrospectValue: Sized {
    /// Type-tag string used in the schema (`"int"`, `"bool"`, ...).
    const TYPE_TAG: &'static str;

    /// Convert a copy of the current value to the wire form.
    fn to_introspect_value(&self) -> IntrospectValue;

    /// Pull `value` back into `Self`. `TypeMismatch` when the wire
    /// variant does not match `TYPE_TAG`.
    ///
    /// # Errors
    /// Returns [`InterveneError::TypeMismatch`] when `value`'s variant is
    /// incompatible with `Self` or carries a payload outside `Self`'s range.
    fn from_introspect_value(value: IntrospectValue) -> Result<Self, InterveneError>;
}

impl IntoIntrospectValue for i32 {
    const TYPE_TAG: &'static str = "int";
    fn to_introspect_value(&self) -> IntrospectValue {
        IntrospectValue::Int(i64::from(*self))
    }
    fn from_introspect_value(value: IntrospectValue) -> Result<Self, InterveneError> {
        if let IntrospectValue::Int(n) = value {
            i32::try_from(n).map_err(|_| InterveneError::TypeMismatch)
        } else {
            Err(InterveneError::TypeMismatch)
        }
    }
}

impl IntoIntrospectValue for i64 {
    const TYPE_TAG: &'static str = "int";
    fn to_introspect_value(&self) -> IntrospectValue {
        IntrospectValue::Int(*self)
    }
    fn from_introspect_value(value: IntrospectValue) -> Result<Self, InterveneError> {
        if let IntrospectValue::Int(n) = value {
            Ok(n)
        } else {
            Err(InterveneError::TypeMismatch)
        }
    }
}

impl IntoIntrospectValue for f64 {
    const TYPE_TAG: &'static str = "float";
    fn to_introspect_value(&self) -> IntrospectValue {
        IntrospectValue::Float(*self)
    }
    fn from_introspect_value(value: IntrospectValue) -> Result<Self, InterveneError> {
        if let IntrospectValue::Float(x) = value {
            Ok(x)
        } else {
            Err(InterveneError::TypeMismatch)
        }
    }
}

impl IntoIntrospectValue for bool {
    const TYPE_TAG: &'static str = "bool";
    fn to_introspect_value(&self) -> IntrospectValue {
        IntrospectValue::Bool(*self)
    }
    fn from_introspect_value(value: IntrospectValue) -> Result<Self, InterveneError> {
        if let IntrospectValue::Bool(b) = value {
            Ok(b)
        } else {
            Err(InterveneError::TypeMismatch)
        }
    }
}

impl IntoIntrospectValue for String {
    const TYPE_TAG: &'static str = "string";
    fn to_introspect_value(&self) -> IntrospectValue {
        IntrospectValue::Text(self.clone())
    }
    fn from_introspect_value(value: IntrospectValue) -> Result<Self, InterveneError> {
        if let IntrospectValue::Text(s) = value {
            Ok(s)
        } else {
            Err(InterveneError::TypeMismatch)
        }
    }
}

/// `External` node that surfaces a `Signal<T>` at the introspect path `value`.
/// Drops into the `Scene` tree as `Scene::External(Box::new(SignalExternal::new(s)))`
/// so `/external/value` query/rewind reads and writes the underlying signal.
#[derive(Debug)]
pub struct SignalExternal<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + IntoIntrospectValue + 'static,
{
    signal: Signal<T>,
}

impl<T> SignalExternal<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + IntoIntrospectValue + 'static,
{
    /// Wrap `signal` for RPC introspection. The signal handle is cloned, so
    /// other code can keep observing/mutating the same cell.
    #[must_use]
    pub fn new(signal: Signal<T>) -> Self {
        Self { signal }
    }

    /// Borrow the underlying signal — useful for view-fn read of the same
    /// cell exposed to RPC.
    #[must_use]
    pub fn signal(&self) -> &Signal<T> {
        &self.signal
    }
}

impl<T> External for SignalExternal<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + IntoIntrospectValue + std::fmt::Debug + 'static,
{
    fn backends(&self) -> BackendSupport {
        BackendSupport::new(&[Backend::Gui, Backend::Tui, Backend::Rpc], BackendFallback::Skip)
    }

    fn repaint_ownership(&self) -> RepaintOwner {
        RepaintOwner::Framework
    }

    fn thread_ownership(&self) -> ThreadOwnership {
        ThreadOwnership::UiThreadSync
    }

    fn handles_event(&self, _event: &Event) -> bool {
        false
    }

    fn introspect(&self) -> Option<&dyn ExternalIntrospect> {
        Some(self)
    }

    fn introspect_mut(&mut self) -> Option<&mut dyn ExternalIntrospect> {
        Some(self)
    }
}

impl<T> ExternalIntrospect for SignalExternal<T>
where
    T: Clone + PartialEq + Serialize + DeserializeOwned + IntoIntrospectValue + std::fmt::Debug + 'static,
{
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(schema_fields::<T>())
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        if path == "value" {
            Some(self.signal.get().to_introspect_value())
        } else {
            None
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        if path != "value" {
            return Err(InterveneError::UnknownPath);
        }
        let new = T::from_introspect_value(value)?;
        self.signal.set(new);
        Ok(())
    }
}

/// Compile-time schema field lookup. One `'static` slice per scalar type so
/// `IntrospectSchema::new` keeps its `&'static [...]` contract.
fn schema_fields<T: IntoIntrospectValue>() -> &'static [(&'static str, &'static str)] {
    // Match against the TYPE_TAG constant: same slice for the same tag, so
    // distinct T sharing a tag (e.g. i32 / i64 both -> "int") still resolve
    // to a single `'static` schema entry.
    match T::TYPE_TAG {
        "int" => &[("value", "int")],
        "float" => &[("value", "float")],
        "bool" => &[("value", "bool")],
        "string" => &[("value", "string")],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::owner::Owner;

    #[test]
    fn query_reads_i32_signal_as_int() {
        let s = Signal::new(7_i32);
        let ext = SignalExternal::new(s.clone());
        assert_eq!(ext.query("value"), Some(IntrospectValue::Int(7)));
        s.set(42);
        assert_eq!(ext.query("value"), Some(IntrospectValue::Int(42)));
    }

    #[test]
    fn query_reads_bool_signal_as_bool() {
        let s = Signal::new(true);
        let ext = SignalExternal::new(s);
        assert_eq!(ext.query("value"), Some(IntrospectValue::Bool(true)));
    }

    #[test]
    fn query_reads_string_signal_as_text() {
        let s = Signal::new(String::from("hello"));
        let ext = SignalExternal::new(s);
        assert_eq!(ext.query("value"), Some(IntrospectValue::Text(String::from("hello"))));
    }

    #[test]
    fn query_reads_f64_signal_as_float() {
        let s = Signal::new(2.5_f64);
        let ext = SignalExternal::new(s);
        assert_eq!(ext.query("value"), Some(IntrospectValue::Float(2.5)));
    }

    #[test]
    fn query_unknown_path_returns_none() {
        let s = Signal::new(1_i32);
        let ext = SignalExternal::new(s);
        assert_eq!(ext.query("nope"), None);
    }

    #[test]
    fn intervene_writes_value_back_into_signal() {
        let s = Signal::new(0_i32);
        let mut ext = SignalExternal::new(s.clone());
        ext.intervene("value", IntrospectValue::Int(99)).unwrap();
        assert_eq!(s.get(), 99);
    }

    #[test]
    fn intervene_dirties_subscribed_owner() {
        let s = Signal::new(0_i32);
        let owner = Owner::new();
        owner.run(|| {
            let _ = s.get();
        });
        let mut ext = SignalExternal::new(s);
        ext.intervene("value", IntrospectValue::Int(1)).unwrap();
        assert!(owner.is_dirty());
    }

    #[test]
    fn intervene_unknown_path_errors() {
        let s = Signal::new(0_i32);
        let mut ext = SignalExternal::new(s);
        assert_eq!(
            ext.intervene("bad", IntrospectValue::Int(1)),
            Err(InterveneError::UnknownPath)
        );
    }

    #[test]
    fn intervene_type_mismatch_errors() {
        let s = Signal::new(0_i32);
        let mut ext = SignalExternal::new(s);
        assert_eq!(
            ext.intervene("value", IntrospectValue::Bool(true)),
            Err(InterveneError::TypeMismatch)
        );
    }

    #[test]
    fn intervene_i32_out_of_range_errors() {
        let s = Signal::new(0_i32);
        let mut ext = SignalExternal::new(s.clone());
        let too_big = i64::from(i32::MAX) + 1;
        assert_eq!(
            ext.intervene("value", IntrospectValue::Int(too_big)),
            Err(InterveneError::TypeMismatch)
        );
        // Signal untouched.
        assert_eq!(s.get(), 0);
    }

    #[test]
    fn schema_declares_value_field_with_typed_tag() {
        let s_int = Signal::new(0_i32);
        let ext_int = SignalExternal::new(s_int);
        assert_eq!(ext_int.schema().fields, &[("value", "int")]);

        let s_bool = Signal::new(false);
        let ext_bool = SignalExternal::new(s_bool);
        assert_eq!(ext_bool.schema().fields, &[("value", "bool")]);

        let s_text = Signal::new(String::new());
        let ext_text = SignalExternal::new(s_text);
        assert_eq!(ext_text.schema().fields, &[("value", "string")]);

        let s_f = Signal::new(0.0_f64);
        let ext_f = SignalExternal::new(s_f);
        assert_eq!(ext_f.schema().fields, &[("value", "float")]);
    }

    #[test]
    fn external_introspect_round_trips_via_external_trait() {
        let s = Signal::new(0_i32);
        let mut ext: Box<dyn External> = Box::new(SignalExternal::new(s.clone()));
        let read = ext.introspect().expect("opted in").query("value");
        assert_eq!(read, Some(IntrospectValue::Int(0)));
        ext.introspect_mut()
            .expect("opted in")
            .intervene("value", IntrospectValue::Int(5))
            .unwrap();
        assert_eq!(s.get(), 5);
    }
}
