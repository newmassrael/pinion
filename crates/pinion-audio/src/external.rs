//! The AI-first audio surface: an introspectable [`AudioEngineExternal`].
//!
//! This is the §5.54 requirement met the pinion way — the audio graph is
//! exposed over the §5.15 introspection triad (§2 #2 / #7), NOT hidden
//! behind an opaque handle. An agent reads *what is playing* and drives
//! *play/stop* over RPC:
//!
//! - **query** — `voice_count` / `master_gain` / `sample_rate` / `voices`
//!   (per-voice id/label/gain/pan/loop/position + resolved 3D
//!   position/distance/effective gain/pan) / `clips` (the library) /
//!   `listener` / `attenuation` (the 3D listener + distance model).
//! - **intervene** — set `master_gain` / `listener` / `attenuation`.
//! - **invoke** — `play` a named clip (optionally at a 3D `position`), `stop`
//!   a voice id, `stop_all`.
//!
//! It holds a shared [`AudioEngine`] plus a named clip library, so `play`
//! takes a clip *name* (an agent cannot pass PCM over the wire).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, int_of,
};
use pinion_core::intent::Intent;
use serde::Serialize;

use crate::clip::AudioClip;
use crate::engine::{AudioEngine, PlayOptions};
use crate::rt::AudioController;
use crate::spatial::{Attenuation, Listener, Vec3};

/// A named clip library an agent `play`s by name — an agent cannot pass PCM
/// over the wire, so both audio Externals resolve a name to shared decoded
/// PCM through this one store.
#[derive(Debug, Default)]
struct ClipLibrary {
    clips: BTreeMap<String, Arc<AudioClip>>,
}

impl ClipLibrary {
    /// Register `clip` under `name` (replacing any prior entry).
    fn insert(&mut self, name: impl Into<String>, clip: Arc<AudioClip>) {
        self.clips.insert(name.into(), clip);
    }

    /// The shared clip registered under `name`, or `None` — the caller maps
    /// the miss to its own "no such clip" error.
    fn get(&self, name: &str) -> Option<Arc<AudioClip>> {
        self.clips.get(name).cloned()
    }

    /// The registered clip names (the `clips` query surface).
    fn names(&self) -> Vec<&str> {
        self.clips.keys().map(String::as_str).collect()
    }
}

/// Introspectable `External` over a shared audio engine + clip library.
#[derive(Debug)]
pub struct AudioEngineExternal {
    engine: Rc<RefCell<AudioEngine>>,
    clips: ClipLibrary,
    pending_intents: Vec<Intent>,
}

impl AudioEngineExternal {
    /// Wrap a shared engine with an empty clip library.
    #[must_use]
    pub fn new(engine: Rc<RefCell<AudioEngine>>) -> Self {
        Self {
            engine,
            clips: ClipLibrary::default(),
            pending_intents: Vec::new(),
        }
    }

    /// Register a named clip agents / keys can `play` by name.
    #[must_use]
    pub fn with_clip(mut self, name: impl Into<String>, clip: Arc<AudioClip>) -> Self {
        self.clips.insert(name, clip);
        self
    }

    /// Route a keyboard/RPC `send` verb to the engine. The verb is either
    /// the reserved control word `stop_all`, or a **clip name** (one-shot
    /// play). No delimiter is used, so the wire never collides with the
    /// [`split_send_payload`](pinion_core::composite_tag::split_send_payload)
    /// `:` grammar; the reserved word takes precedence over a same-named
    /// clip.
    fn apply_send(&mut self, verb: &str) -> Result<IntrospectValue, InvokeError> {
        if verb == "stop_all" {
            self.engine.borrow_mut().stop_all();
            return Ok(IntrospectValue::Null);
        }
        let clip = self.clips.get(verb).ok_or(InvokeError::Rejected)?;
        let id = self
            .engine
            .borrow_mut()
            .play(clip, verb.to_string(), PlayOptions::one_shot());
        self.pending_intents.push(Intent::new_static(
            "audio.play",
            IntrospectValue::Text(verb.to_string()),
        ));
        Ok(IntrospectValue::Int(int_of_u64(id)))
    }

    /// Apply a per-voice `intervene` (`voice.<id>.{gain,pan,position}`). An
    /// unknown voice id or field is `UnknownPath`; the wrong value type is
    /// `TypeMismatch`. `position` accepts a `[x, y, z]` array (place in 3D) or
    /// `null` (un-spatialise back to the authored pan).
    fn intervene_voice(
        &mut self,
        id: u64,
        field: &str,
        value: IntrospectValue,
    ) -> Result<(), InterveneError> {
        let mut engine = self.engine.borrow_mut();
        let matched = match (field, value) {
            ("gain", IntrospectValue::Float(g)) => engine.set_voice_gain(id, f32_of(g)),
            ("pan", IntrospectValue::Float(p)) => engine.set_voice_pan(id, f32_of(p)),
            ("position", IntrospectValue::Null) => engine.set_voice_position(id, None),
            ("position", IntrospectValue::Json(v)) => match parse_vec3(Some(&v)) {
                Some(pos) => engine.set_voice_position(id, Some(pos)),
                None => return Err(InterveneError::TypeMismatch),
            },
            ("gain" | "pan" | "position", _) => return Err(InterveneError::TypeMismatch),
            _ => return Err(InterveneError::UnknownPath),
        };
        if matched {
            Ok(())
        } else {
            // No live voice with that id.
            Err(InterveneError::UnknownPath)
        }
    }
}

/// Split a `voice.<id>.<field>` intervene path into `(id, field)`.
fn parse_voice_path(path: &str) -> Option<(u64, &str)> {
    let (id_str, field) = path.strip_prefix("voice.")?.split_once('.')?;
    Some((id_str.parse().ok()?, field))
}

/// The per-voice shape emitted by the `voices` query. `gain`/`pan` are the
/// *authored* values; `effective_gain`/`effective_pan` are what the mixer
/// actually renders after spatialisation (equal to the authored values for a
/// flat voice). `position`/`distance` are present only for a 3D voice.
#[derive(Serialize)]
struct VoiceInfo<'a> {
    id: u64,
    label: &'a str,
    gain: f32,
    pan: f32,
    looping: bool,
    position_secs: f32,
    finished: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<Vec3>,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance: Option<f32>,
    effective_gain: f32,
    effective_pan: f32,
}

// Paints nothing (the binding's `view` is the paint scene) and emits §5.20
// intents — the RPC-only read-write introspection skeleton.
pinion_core::intent_query_external_impl!(AudioEngineExternal);

impl ExternalIntrospect for AudioEngineExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("voice_count", "int"),
            ("sample_rate", "int"),
            ("master_gain", "float"),
            ("voices", "json"),
            ("clips", "json"),
            ("listener", "json"),
            ("attenuation", "json"),
            // Per-voice writes (the read twins are the `voices` array fields).
            ("voice.<id>.gain", "float"),
            ("voice.<id>.pan", "float"),
            ("voice.<id>.position", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let engine = self.engine.borrow();
        match path {
            "voice_count" => Some(IntrospectValue::Int(int_of(engine.voice_count()))),
            "sample_rate" => Some(IntrospectValue::Int(i64::from(engine.sample_rate()))),
            "master_gain" => Some(IntrospectValue::Float(f64::from(engine.master_gain()))),
            "voices" => {
                let infos: Vec<VoiceInfo> = engine
                    .voices()
                    .map(|(id, v)| {
                        let resolved = engine.resolve_voice(v);
                        VoiceInfo {
                            id,
                            label: v.label(),
                            gain: v.gain(),
                            pan: v.pan(),
                            looping: v.looping(),
                            position_secs: v.position_secs(),
                            finished: v.is_finished(),
                            position: v.position(),
                            distance: resolved.distance,
                            effective_gain: resolved.gain,
                            effective_pan: resolved.pan,
                        }
                    })
                    .collect();
                Some(IntrospectValue::json(&infos))
            }
            "clips" => Some(IntrospectValue::json(&self.clips.names())),
            "listener" => Some(IntrospectValue::json(&engine.listener())),
            "attenuation" => Some(IntrospectValue::json(&engine.attenuation())),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        // Per-voice live drive: `voice.<id>.{gain,pan,position}` — the write
        // twins of the per-voice reads in the `voices` query (§2 #7 symmetry).
        if let Some((id, field)) = parse_voice_path(path) {
            return self.intervene_voice(id, field, value);
        }
        match (path, value) {
            ("master_gain", IntrospectValue::Float(g)) => {
                self.engine.borrow_mut().set_master_gain(f32_of(g));
                Ok(())
            }
            // Move / re-orient the listener. Any of position/forward/up may be
            // given; the rest keep their current value.
            ("listener", IntrospectValue::Json(v)) => {
                let mut engine = self.engine.borrow_mut();
                let cur = engine.listener();
                let position = parse_vec3(v.get("position")).unwrap_or(cur.position);
                let forward = parse_vec3(v.get("forward")).unwrap_or(cur.forward);
                let up = parse_vec3(v.get("up")).unwrap_or(cur.up);
                engine.set_listener(Listener::new(position, forward, up));
                Ok(())
            }
            // Tune the distance-attenuation model (any field optional).
            ("attenuation", IntrospectValue::Json(v)) => {
                let mut engine = self.engine.borrow_mut();
                let cur = engine.attenuation();
                let attenuation = Attenuation {
                    reference_distance: parse_f32(v.get("reference_distance"))
                        .unwrap_or(cur.reference_distance),
                    max_distance: parse_f32(v.get("max_distance")).unwrap_or(cur.max_distance),
                    rolloff: parse_f32(v.get("rolloff")).unwrap_or(cur.rolloff),
                };
                engine.set_attenuation(attenuation);
                Ok(())
            }
            // A known path with the wrong value type.
            ("master_gain" | "listener" | "attenuation", _) => Err(InterveneError::TypeMismatch),
            _ => Err(InterveneError::UnknownPath),
        }
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // The keyboard `keybinding -> forward -> send` path arrives here
            // with a verb: a clip name (play) or the reserved `stop_all`.
            "send" => match args {
                IntrospectValue::Text(verb) => self.apply_send(&verb),
                _ => Err(InvokeError::TypeMismatch),
            },
            "play" => {
                let (name, opts) = parse_play(args)?;
                let clip = self.clips.get(&name).ok_or(InvokeError::Rejected)?;
                let id = self.engine.borrow_mut().play(clip, name.clone(), opts);
                self.pending_intents.push(Intent::new_static(
                    "audio.play",
                    IntrospectValue::Text(name),
                ));
                Ok(IntrospectValue::Int(int_of_u64(id)))
            }
            "stop" => match args {
                IntrospectValue::Int(n) => {
                    let stopped = self.engine.borrow_mut().stop(u64_of(n));
                    Ok(IntrospectValue::Bool(stopped))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "stop_all" => {
                self.engine.borrow_mut().stop_all();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

/// Introspectable `External` over the real-time [`AudioController`] + a clip
/// library — the RT **device** path on the same §2 #7 RPC surface as
/// [`AudioEngineExternal`], so the audio that actually ships is scene-as-data,
/// not an opaque handle (the R1283/R1284 audit's deepest gap).
///
/// The RT read/write shape differs from the single-thread engine's, and
/// honestly so — it is the lock-free split, not a lesser surface:
///
/// - **query** reads the aggregate the audio thread publishes each render
///   ([`crate::AudioSnapshot`]): `voice_count` / `peak` / `frames_rendered` /
///   `rejected`. This is the "observe the RT audio without touching its
///   engine" seam. The per-voice detail [`AudioEngineExternal`] can walk is
///   deliberately absent — a full lock-free per-voice snapshot is a later
///   refinement, not part of this slice.
/// - **invoke** drives the audio thread over the command queue: `play` a named
///   clip, `stop` a voice id, `stop_all`. These are fire-and-forget commands.
/// - **intervene** has no paths: the control thread cannot read RT engine
///   state back, so there is no read-backable settable slot to intervene on.
///   Param drive (master / voice gain, listener, attenuation) will arrive as
///   `invoke` actions alongside the richer snapshot, not as `intervene`.
#[derive(Debug)]
pub struct AudioControllerExternal {
    controller: AudioController,
    clips: ClipLibrary,
    pending_intents: Vec<Intent>,
}

impl AudioControllerExternal {
    /// Wrap a real-time controller with an empty clip library.
    #[must_use]
    pub fn new(controller: AudioController) -> Self {
        Self {
            controller,
            clips: ClipLibrary::default(),
            pending_intents: Vec::new(),
        }
    }

    /// Register a named clip agents can `play` by name.
    #[must_use]
    pub fn with_clip(mut self, name: impl Into<String>, clip: Arc<AudioClip>) -> Self {
        self.clips.insert(name, clip);
        self
    }

    /// The wrapped control-thread handle — e.g. to `reclaim` retired voices on
    /// an idle frame, or poll the snapshot directly.
    pub fn controller_mut(&mut self) -> &mut AudioController {
        &mut self.controller
    }
}

// RPC-only introspection skeleton (paints nothing; emits §5.20 intents).
pinion_core::intent_query_external_impl!(AudioControllerExternal);

impl ExternalIntrospect for AudioControllerExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("voice_count", "int"),
            ("peak", "float"),
            ("frames_rendered", "int"),
            ("rejected", "int"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let snapshot = self.controller.snapshot();
        match path {
            "voice_count" => Some(IntrospectValue::Int(i64::from(snapshot.voice_count()))),
            "peak" => Some(IntrospectValue::Float(f64::from(snapshot.peak()))),
            "frames_rendered" => Some(IntrospectValue::Int(int_of_u64(snapshot.frames_rendered()))),
            "rejected" => Some(IntrospectValue::Int(int_of_u64(snapshot.rejected()))),
            _ => None,
        }
    }

    /// The RT path exposes no read-backable settable state, so every path is
    /// unknown here; drive it through `invoke` instead.
    fn intervene(&mut self, _path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(InterveneError::UnknownPath)
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "play" => {
                let (name, opts) = parse_play(args)?;
                let clip = self.clips.get(&name).ok_or(InvokeError::Rejected)?;
                // `Rejected` also covers a full command ring (the play could
                // not be queued) — both are "could not play now".
                let id = self
                    .controller
                    .play(clip, name.clone(), opts)
                    .ok_or(InvokeError::Rejected)?;
                self.pending_intents.push(Intent::new_static(
                    "audio.play",
                    IntrospectValue::Text(name),
                ));
                Ok(IntrospectValue::Int(int_of_u64(id)))
            }
            // The bool is whether the command was *queued* (the actual stop
            // happens on the audio thread next render), not whether a voice
            // matched — the control thread cannot know that synchronously.
            "stop" => match args {
                IntrospectValue::Int(n) => {
                    Ok(IntrospectValue::Bool(self.controller.stop(u64_of(n))))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "stop_all" => {
                self.controller.stop_all();
                Ok(IntrospectValue::Null)
            }
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn parse_play(args: IntrospectValue) -> Result<(String, PlayOptions), InvokeError> {
    match args {
        IntrospectValue::Text(name) => Ok((name, PlayOptions::one_shot())),
        IntrospectValue::Json(v) => {
            let name = v
                .get("name")
                .and_then(serde_json::Value::as_str)
                .ok_or(InvokeError::TypeMismatch)?
                .to_string();
            let mut opts = PlayOptions::one_shot();
            if let Some(g) = v.get("gain").and_then(serde_json::Value::as_f64) {
                opts.gain = f32_of(g);
            }
            if let Some(p) = v.get("pan").and_then(serde_json::Value::as_f64) {
                opts.pan = f32_of(p);
            }
            if let Some(l) = v.get("looping").and_then(serde_json::Value::as_bool) {
                opts.looping = l;
            }
            if let Some(pos) = parse_vec3(v.get("position")) {
                opts.position = Some(pos);
            }
            Ok((name, opts))
        }
        _ => Err(InvokeError::TypeMismatch),
    }
}

/// Parse an optional JSON `[x, y, z]` array into a [`Vec3`].
fn parse_vec3(v: Option<&serde_json::Value>) -> Option<Vec3> {
    let arr = v?.as_array()?;
    let [x, y, z] = arr.as_slice() else {
        return None;
    };
    Some([
        f32_of(x.as_f64()?),
        f32_of(y.as_f64()?),
        f32_of(z.as_f64()?),
    ])
}

/// Parse an optional JSON number into an `f32`.
fn parse_f32(v: Option<&serde_json::Value>) -> Option<f32> {
    Some(f32_of(v?.as_f64()?))
}

fn int_of_u64(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn u64_of(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

#[allow(clippy::cast_possible_truncation)] // audio gains/pans lose no meaningful precision as f32.
fn f32_of(x: f64) -> f32 {
    x as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use pinion_core::external::External;

    fn director() -> AudioEngineExternal {
        let engine = Rc::new(RefCell::new(AudioEngine::new(48_000)));
        let bell = AudioClip::new(48_000, 1, vec![1.0; 8]).shared();
        let waves = AudioClip::new(48_000, 1, vec![0.5; 8]).shared();
        AudioEngineExternal::new(engine)
            .with_clip("bell", bell)
            .with_clip("waves", waves)
    }

    #[test]
    fn play_named_clip_then_query_voices() {
        let mut ext = director();
        let id = ext.invoke("play", IntrospectValue::Text("bell".to_string()));
        assert!(matches!(id, Ok(IntrospectValue::Int(1))));
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));
        assert!(ext.is_dirty(), "play queues an intent");

        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert_eq!(items[0]["label"], "bell");
                assert_eq!(items[0]["looping"], false);
            }
            other => panic!("expected voices array, got {other:?}"),
        }
    }

    #[test]
    fn play_unknown_clip_is_rejected() {
        let mut ext = director();
        assert!(matches!(
            ext.invoke("play", IntrospectValue::Text("nope".to_string())),
            Err(InvokeError::Rejected)
        ));
    }

    #[test]
    fn play_json_opts_and_stop() {
        let mut ext = director();
        let args = IntrospectValue::Json(serde_json::json!({
            "name": "waves", "gain": 0.3, "looping": true
        }));
        let id = match ext.invoke("play", args) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected id, got {other:?}"),
        };
        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert_eq!(items[0]["looping"], true);
                assert!((items[0]["gain"].as_f64().unwrap() - 0.3).abs() < 1e-3);
            }
            other => panic!("expected voices, got {other:?}"),
        }
        assert!(matches!(
            ext.invoke("stop", IntrospectValue::Int(id)),
            Ok(IntrospectValue::Bool(true))
        ));
    }

    #[test]
    fn send_wire_plays_by_clip_name_and_stops() {
        let mut ext = director();
        // A bare clip name plays it (no delimiter → no `:` grammar clash).
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Text("bell".to_string())),
            Ok(IntrospectValue::Int(_))
        ));
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));
        ext.invoke("send", IntrospectValue::Text("stop_all".to_string()))
            .expect("stop_all sends");
        assert!(matches!(
            ext.invoke("send", IntrospectValue::Text("nope".to_string())),
            Err(InvokeError::Rejected)
        ));
    }

    #[test]
    fn master_gain_intervene_roundtrips() {
        let mut ext = director();
        ext.intervene("master_gain", IntrospectValue::Float(0.25))
            .expect("set master");
        assert!(matches!(
            ext.query("master_gain"),
            Some(IntrospectValue::Float(g)) if (g - 0.25).abs() < 1e-6
        ));
    }

    #[test]
    fn play_at_position_exposes_resolved_spatial_reads() {
        let mut ext = director();
        // Play the bell to the world-right at twice the reference distance.
        let args = IntrospectValue::Json(serde_json::json!({
            "name": "bell", "position": [2.0, 0.0, 0.0]
        }));
        ext.invoke("play", args).expect("spatial play");

        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                let v = &items[0];
                assert_eq!(v["position"], serde_json::json!([2.0, 0.0, 0.0]));
                assert!((v["distance"].as_f64().unwrap() - 2.0).abs() < 1e-4);
                // gain 1.0 halved at 2× reference; pan resolves hard right.
                assert!((v["effective_gain"].as_f64().unwrap() - 0.5).abs() < 1e-3);
                assert!((v["effective_pan"].as_f64().unwrap() - 1.0).abs() < 1e-3);
            }
            other => panic!("expected voices array, got {other:?}"),
        }
    }

    #[test]
    fn listener_query_and_intervene_roundtrip() {
        let mut ext = director();
        // Default listener faces -Z from the origin.
        match ext.query("listener") {
            Some(IntrospectValue::Json(v)) => {
                assert_eq!(v["position"], serde_json::json!([0.0, 0.0, 0.0]));
                assert_eq!(v["forward"], serde_json::json!([0.0, 0.0, -1.0]));
            }
            other => panic!("expected listener json, got {other:?}"),
        }
        // Move it; a partial object keeps the unspecified axes.
        ext.intervene(
            "listener",
            IntrospectValue::Json(serde_json::json!({ "position": [5.0, 0.0, 0.0] })),
        )
        .expect("move listener");
        match ext.query("listener") {
            Some(IntrospectValue::Json(v)) => {
                assert_eq!(v["position"], serde_json::json!([5.0, 0.0, 0.0]));
                assert_eq!(
                    v["forward"],
                    serde_json::json!([0.0, 0.0, -1.0]),
                    "forward kept"
                );
            }
            other => panic!("expected listener json, got {other:?}"),
        }
    }

    #[test]
    fn per_voice_intervene_drives_gain_and_position() {
        let mut ext = director();
        let id = match ext.invoke("play", IntrospectValue::Text("bell".to_string())) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected id, got {other:?}"),
        };
        let path = format!("voice.{id}.gain");

        // Fade the one voice — the write twin of the `voices[].gain` read.
        ext.intervene(&path, IntrospectValue::Float(0.3))
            .expect("set voice gain");
        // Place it in 3D to the world-right at 2× reference distance.
        ext.intervene(
            &format!("voice.{id}.position"),
            IntrospectValue::Json(serde_json::json!([2.0, 0.0, 0.0])),
        )
        .expect("set voice position");

        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                let v = &items[0];
                assert!(
                    (v["gain"].as_f64().unwrap() - 0.3).abs() < 1e-3,
                    "authored gain set"
                );
                assert!((v["distance"].as_f64().unwrap() - 2.0).abs() < 1e-4);
                // effective = authored 0.3 × distance atten 0.5 = 0.15.
                assert!((v["effective_gain"].as_f64().unwrap() - 0.15).abs() < 1e-3);
                assert!((v["effective_pan"].as_f64().unwrap() - 1.0).abs() < 1e-3);
            }
            other => panic!("expected voices, got {other:?}"),
        }

        // `null` un-spatialises back to the authored pan (no distance).
        ext.intervene(&format!("voice.{id}.position"), IntrospectValue::Null)
            .expect("clear position");
        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert!(items[0].get("distance").is_none(), "flat again");
            }
            other => panic!("expected voices, got {other:?}"),
        }

        // Unknown voice id / field → UnknownPath; wrong type → TypeMismatch.
        assert!(matches!(
            ext.intervene("voice.999.gain", IntrospectValue::Float(1.0)),
            Err(InterveneError::UnknownPath)
        ));
        assert!(matches!(
            ext.intervene(&path, IntrospectValue::Text("loud".to_string())),
            Err(InterveneError::TypeMismatch)
        ));
    }

    #[test]
    fn attenuation_intervene_changes_resolved_gain() {
        let mut ext = director();
        // Zero rolloff → no distance falloff, so a far voice stays full gain.
        ext.intervene(
            "attenuation",
            IntrospectValue::Json(serde_json::json!({ "rolloff": 0.0 })),
        )
        .expect("set attenuation");
        let args = IntrospectValue::Json(serde_json::json!({
            "name": "bell", "position": [0.0, 0.0, -50.0]
        }));
        ext.invoke("play", args).expect("spatial play");
        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert!(
                    (items[0]["effective_gain"].as_f64().unwrap() - 1.0).abs() < 1e-3,
                    "zero rolloff → no attenuation even at distance 50"
                );
            }
            other => panic!("expected voices, got {other:?}"),
        }
    }

    // --- The real-time device path over the same §2 #7 surface. ---

    fn rt_director(max_voices: usize) -> (AudioControllerExternal, crate::rt::AudioRenderer) {
        let (controller, renderer) =
            crate::rt::realtime_channel(AudioEngine::new(48_000), 16, max_voices);
        // A tone longer than one render block so it stays live to be observed.
        let bell = AudioClip::new(48_000, 1, vec![1.0; 4096]).shared();
        let ext = AudioControllerExternal::new(controller).with_clip("bell", bell);
        (ext, renderer)
    }

    #[test]
    fn rt_invoke_play_then_query_published_snapshot() {
        let (mut ext, mut renderer) = rt_director(8);
        // Nothing rendered yet.
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(0))
        ));

        let id = ext.invoke("play", IntrospectValue::Text("bell".to_string()));
        assert!(
            matches!(id, Ok(IntrospectValue::Int(1))),
            "control-minted id"
        );
        assert!(ext.is_dirty(), "play queues an intent");

        // The command is queued; the audio thread must render to apply it and
        // publish the snapshot the External reads.
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);

        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));
        match ext.query("peak") {
            Some(IntrospectValue::Float(p)) => assert!(p > 0.5, "audible: {p}"),
            other => panic!("expected peak float, got {other:?}"),
        }
        assert!(matches!(
            ext.query("frames_rendered"),
            Some(IntrospectValue::Int(128))
        ));
    }

    #[test]
    fn rt_invoke_stop_all_silences_next_render() {
        let (mut ext, mut renderer) = rt_director(8);
        ext.invoke("play", IntrospectValue::Text("bell".to_string()))
            .expect("play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(1))
        ));

        ext.invoke("stop_all", IntrospectValue::Null)
            .expect("stop_all");
        renderer.render(&mut out);
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(0))
        ));
    }

    #[test]
    fn rt_stop_by_id_reports_queued() {
        let (mut ext, mut renderer) = rt_director(8);
        let id = match ext.invoke("play", IntrospectValue::Text("bell".to_string())) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected id, got {other:?}"),
        };
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        // The bool is "command queued", not "voice matched" (async stop).
        assert!(matches!(
            ext.invoke("stop", IntrospectValue::Int(id)),
            Ok(IntrospectValue::Bool(true))
        ));
    }

    #[test]
    fn rt_rejected_metric_surfaces_voice_starvation() {
        // Pool of 2, three plays → one rejected, visible over the RPC query.
        let (mut ext, mut renderer) = rt_director(2);
        for _ in 0..3 {
            ext.invoke("play", IntrospectValue::Text("bell".to_string()))
                .expect("play queued");
        }
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(2))
        ));
        assert!(
            matches!(ext.query("rejected"), Some(IntrospectValue::Int(1))),
            "the voice-budget drop is introspectable"
        );
    }

    #[test]
    fn rt_unknown_clip_and_paths_are_rejected() {
        let (mut ext, _renderer) = rt_director(8);
        assert!(matches!(
            ext.invoke("play", IntrospectValue::Text("nope".to_string())),
            Err(InvokeError::Rejected)
        ));
        assert!(matches!(
            ext.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        ));
        // The RT path has no intervene slots; query of an unknown field is None.
        assert!(matches!(
            ext.intervene("voice_count", IntrospectValue::Float(1.0)),
            Err(InterveneError::UnknownPath)
        ));
        assert!(ext.query("nonexistent").is_none());
    }
}
