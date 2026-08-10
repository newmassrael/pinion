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
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, SchemaArg,
    SchemaField, int_of, read_only_or_unknown,
};
use pinion_core::intent::Intent;
use serde::Serialize;

use crate::clip::AudioClip;
use crate::engine::{AudioEngine, PlayOptions, VoicePolicy};
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
        let clip = self
            .clips
            .get(verb)
            .ok_or_else(|| unknown_clip("send", verb))?;
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

/// The per-voice shape emitted by the real-time `voices` query. Assembled from
/// the lock-free [`crate::AudioSnapshot`] slots (the numbers) + the
/// control-thread label map (the `String`). Carries authored `gain`/`pan` and
/// `effective_gain`/`effective_pan`, matching the single-thread [`VoiceInfo`]
/// shape. The only honest difference: no `finished` flag — a finished voice is
/// reaped before the next publish, so only live voices appear (the RT model has
/// no "finished-but-listed" transient).
#[derive(Serialize)]
struct RtVoiceInfo<'a> {
    id: u64,
    label: &'a str,
    gain: f32,
    pan: f32,
    effective_gain: f32,
    effective_pan: f32,
    position_secs: f32,
    looping: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    position: Option<Vec3>,
    #[serde(skip_serializing_if = "Option::is_none")]
    distance: Option<f32>,
}

// Paints nothing (the binding's `view` is the paint scene) and emits §5.20
// intents — the RPC-only read-write introspection skeleton.
pinion_core::intent_query_external_impl!(AudioEngineExternal);

/// R1637 §5.15 — every path the retained single-thread engine answers, as a
/// `const` a WRAPPER can compose with [`SchemaField::concat`].
///
/// The RT surface next door has had [`RT_EXTERNAL_FIELDS`] since R1501 for
/// exactly this reason; this one did not, so `hello-audio-rt`'s harness could
/// only forward the declaration verbatim and its own `render` verb — the one
/// the whole deterministic demo is stepped by — was callable and unpublished.
pub const ENGINE_EXTERNAL_FIELDS: &[SchemaField] = &[
    SchemaField::new("voice_count", "int"),
    SchemaField::new("sample_rate", "int"),
    SchemaField::new("master_gain", "float"),
    SchemaField::new("voices", "json"),
    SchemaField::new("clips", "json"),
    SchemaField::new("listener", "json"),
    SchemaField::new("attenuation", "json"),
    // R1637 — the action channel, declared. These four answered
    // over the wire from R1274 onward while `$schema` listed
    // only the reads, so an agent could observe this engine and
    // had to be TOLD how to drive it. `send` is the keybinding
    // forwarder's verb (a clip name, or the reserved
    // `stop_all`); it is on the wire, so it is declared here.
    SchemaField::action("send", "int"),
    SchemaField::action("play", "int"),
    SchemaField::action("stop", "bool"),
    SchemaField::action("stop_all", "null"),
    // Per-voice writes (the read twins are the `voices` array fields).
    SchemaField::parametric(
        "voice.<id>.gain",
        "float",
        const { &[SchemaArg::key("id", "int", "voices")] },
    ),
    SchemaField::parametric(
        "voice.<id>.pan",
        "float",
        const { &[SchemaArg::key("id", "int", "voices")] },
    ),
    SchemaField::parametric(
        "voice.<id>.position",
        "json",
        const { &[SchemaArg::key("id", "int", "voices")] },
    ),
];

impl ExternalIntrospect for AudioEngineExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(ENGINE_EXTERNAL_FIELDS)
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
                let listener = listener_from_partial(engine.listener(), &v);
                engine.set_listener(listener);
                Ok(())
            }
            // Tune the distance-attenuation model (any field optional).
            ("attenuation", IntrospectValue::Json(v)) => {
                let mut engine = self.engine.borrow_mut();
                let attenuation = attenuation_from_partial(engine.attenuation(), &v);
                engine.set_attenuation(attenuation);
                Ok(())
            }
            // A known path with the wrong value type.
            ("master_gain" | "listener" | "attenuation", _) => Err(InterveneError::TypeMismatch),
            // A read-only declared field (voice_count/sample_rate/voices/clips)
            // is `ReadOnly`; anything else is genuinely unknown.
            _ => Err(read_only_or_unknown(&self.schema(), path)),
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
                let clip = self
                    .clips
                    .get(&name)
                    .ok_or_else(|| unknown_clip("play", &name))?;
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
/// library — the RT **device** path brought onto the §2 #7 RPC surface, so the
/// audio that actually ships is scene-as-data and drivable over RPC, not an
/// opaque handle (the R1283/R1284 audit's deepest gap).
///
/// The RT read/write shape is the lock-free split, and differs from the
/// single-thread [`AudioEngineExternal`]: reads go through `query`, writes
/// through `invoke`, and `intervene` exposes no writable slots.
///
/// - **query** reads what the audio thread publishes each render
///   ([`crate::AudioSnapshot`]): the aggregate `voice_count` / `peak` /
///   `frames_rendered` / `rejected` / `stolen`, and the per-voice `voices` array
///   (id / label / authored + effective gain+pan / `position_secs` / loop /
///   resolved 3D per live voice) read lock-free off the pre-allocated per-voice
///   slots and joined to the control-thread label map; plus the 3D `listener` /
///   `attenuation` and `voice_policy` read
///   off the control-thread mirror (the audio thread cannot be read lock-free,
///   but the control thread is their sole writer — see
///   [`AudioController::listener`]). This is the "observe the RT audio without
///   touching its engine" seam.
/// - **invoke** drives the audio thread over the command queue: `play` a named
///   clip, `stop` a voice id, `stop_all`, `set_master_gain`, and the per-voice
///   `set_voice_gain` / `set_voice_pan` / `set_voice_position` (move a 3D
///   emitter after it starts, or `null` to un-spatialise), the 3D `set_listener`
///   / `set_attenuation` (each a partial update patched against the mirror), and
///   `set_voice_policy`. All return `Null`, or `Rejected` when the command ring
///   is full (the write did not reach the audio thread) — a full ring is
///   surfaced, never silently dropped.
/// - **intervene** exposes no writable slots: a schema-declared field
///   (`voice_count` … `voices` … `listener` / `attenuation` / `voice_policy`)
///   is a read-only observation, so an intervene of it is `ReadOnly` (drive via
///   `invoke set_listener` etc.) and anything else `UnknownPath`.
///
/// The per-voice `voices` read matches the single-thread engine's shape
/// (authored + effective gain/pan, position/distance) with one honest
/// difference: it lists only *live* voices — a finished one is reaped before the
/// next publish, so there is no `finished` flag (the RT model has no
/// "finished-but-listed" transient the retained single-thread engine keeps).
#[derive(Debug)]
pub struct AudioControllerExternal {
    controller: SharedController,
    clips: ClipLibrary,
    pending_intents: Vec<Intent>,
}

impl AudioControllerExternal {
    /// Wrap a real-time controller with an empty clip library.
    #[must_use]
    pub fn new(controller: AudioController) -> Self {
        Self::from_shared(shared_controller(controller))
    }

    /// Wrap a controller that is **already shared** with another control-thread
    /// driver — the per-frame ticker case (see [`SharedController`]).
    #[must_use]
    pub fn from_shared(controller: SharedController) -> Self {
        Self {
            controller,
            clips: ClipLibrary::default(),
            pending_intents: Vec::new(),
        }
    }

    /// The per-voice parameter verbs — `set_voice_{gain,pan,position}` — which all
    /// take `{ "id": <int>, "<field>": … }`. One home, so the id parse and the
    /// full-ring `Rejected` mapping cannot drift between them.
    fn invoke_voice_param(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        let IntrospectValue::Json(v) = args else {
            return Err(InvokeError::TypeMismatch);
        };
        let id = v
            .get("id")
            .and_then(serde_json::Value::as_u64)
            .ok_or(InvokeError::TypeMismatch)?;
        let mut controller = self.controller.borrow_mut();
        let queued = match path {
            "set_voice_gain" => {
                let gain = parse_f32(v.get("gain")).ok_or(InvokeError::TypeMismatch)?;
                controller.set_voice_gain(id, gain)
            }
            "set_voice_pan" => {
                let pan = parse_f32(v.get("pan")).ok_or(InvokeError::TypeMismatch)?;
                controller.set_voice_pan(id, pan)
            }
            // `null` un-spatialises the voice back to its authored pan. Same wire
            // form a world-routed emitter uses — one parser, one contract.
            "set_voice_position" => {
                let (_, position) = parse_emitter(&v).ok_or(InvokeError::TypeMismatch)?;
                controller.set_voice_position(id, position)
            }
            // Reached only from the three-arm dispatch in `invoke`; a `_ =>`
            // catch-all here would make a 4th verb routed in by mistake a SILENT
            // position write.
            other => unreachable!("invoke_voice_param called with {other:?}"),
        };
        queued_or_rejected(queued)
    }

    /// Register a named clip agents can `play` by name.
    #[must_use]
    pub fn with_clip(mut self, name: impl Into<String>, clip: Arc<AudioClip>) -> Self {
        self.clips.insert(name, clip);
        self
    }

    /// Free the voices the audio thread has returned, on the control thread —
    /// a delegate to [`AudioController::reclaim`]. `invoke` reclaims
    /// opportunistically (every command send does), so a driving agent needs
    /// this only when it *only* queries: call it on an idle frame to keep the
    /// resource-return queue drained. Returns how many were reclaimed.
    pub fn reclaim(&mut self) -> usize {
        self.controller.borrow_mut().reclaim()
    }
}

// RPC-only introspection skeleton (paints nothing; emits §5.20 intents).
//
// §5.15 item 3 — `OwnThread`, not the `UiThreadSync` default: this External exists
// to drive an [`AudioRenderer`] that lives on a device's audio thread, reached only
// over the lock-free ring (that is what `rt` IS). Declaring `UiThreadSync` here
// would publish a falsehood to any consumer that mounts it directly with a real
// device — which is the documented way to use it. A harness that deliberately
// co-locates the renderer on the dispatch thread (`hello-audio-rt`'s step-verb
// wrapper) declares `UiThreadSync` for itself, which is then true of *that* type.
pinion_core::intent_query_external_impl!(AudioControllerExternal, OwnThread);

/// An [`AudioController`] shared between the control-thread drivers that need it.
///
/// [`AudioController`] cannot be cloned — its command ring is SPSC, so there is
/// exactly *one* producer by construction — yet a game has more than one thing
/// that wants to talk to the audio thread from the control side: the RPC / UI
/// surface, and the per-frame world sync (listener follows the camera, emitters
/// follow entities). `Rc<RefCell<_>>` is the honest way to share it: both live on
/// the main thread, and the genuinely *concurrent* party — the audio thread — is
/// on the far side of the lock-free ring, untouched by this borrow.
pub type SharedController = Rc<RefCell<AudioController>>;

/// Put a freshly-built controller behind a [`SharedController`] handle.
#[must_use]
pub fn shared_controller(controller: AudioController) -> SharedController {
    Rc::new(RefCell::new(controller))
}

/// The fields [`AudioControllerExternal`] declares — the RT surface's schema, as
/// data.
///
/// Public so a binding that *wraps* the RT External (a device host adding
/// `device` / `sample_rate` / `channels`, say) can compose its own schema from
/// this list instead of hand-copying it, which would be a second source of truth
/// that silently drifts the moment a field is added here.
pub const RT_EXTERNAL_FIELDS: &[SchemaField] = &[
    SchemaField::new("voice_count", "int"),
    SchemaField::new("max_voices", "int"),
    SchemaField::new("peak", "float"),
    SchemaField::new("frames_rendered", "int"),
    SchemaField::new("rejected", "int"),
    SchemaField::new("stolen", "int"),
    // Per-voice detail read off the lock-free per-voice slots + the
    // control-thread label map.
    SchemaField::new("voices", "json"),
    // The 3D listener / attenuation, read from the control-thread
    // mirror. Query-readable but driven via `invoke set_listener` /
    // `set_attenuation` (the RT surface's read-via-query,
    // write-via-invoke split), so an `intervene` of them is `ReadOnly`.
    SchemaField::new("listener", "json"),
    SchemaField::new("attenuation", "json"),
    // The full-pool policy, read from the control-thread mirror; driven
    // via `invoke set_voice_policy`.
    SchemaField::new("voice_policy", "text"),
    // R1637 — the driving verbs, declared. The type doc above this list
    // described all ten of them in PROSE ("**invoke** drives the audio thread
    // over the command queue: `play` a named clip, `stop` a voice id, …") while
    // the declaration listed only the reads — so the one surface §2 #2 makes an
    // agent's primary path published half the contract, and the other half was
    // readable only by opening this file. A queued command answers `null`
    // rather than a matched-bool: the control thread cannot know synchronously
    // whether the audio thread will match, and a full ring is `Rejected`.
    SchemaField::action("play", "int"),
    SchemaField::action("stop", "null"),
    SchemaField::action("stop_all", "null"),
    SchemaField::action("set_master_gain", "null"),
    SchemaField::action("set_voice_gain", "null"),
    SchemaField::action("set_voice_pan", "null"),
    SchemaField::action("set_voice_position", "null"),
    SchemaField::action("set_listener", "null"),
    SchemaField::action("set_attenuation", "null"),
    SchemaField::action("set_voice_policy", "null"),
];

impl ExternalIntrospect for AudioControllerExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(RT_EXTERNAL_FIELDS)
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        // Held for the whole read: `snapshot` and the label map both borrow from
        // it, and `RtVoiceInfo` borrows its label straight out of that map.
        let controller = self.controller.borrow();
        let snapshot = controller.snapshot();
        match path {
            "voice_count" => Some(IntrospectValue::Int(i64::from(snapshot.voice_count()))),
            "max_voices" => Some(IntrospectValue::Int(int_of(snapshot.max_voices()))),
            "peak" => Some(IntrospectValue::Float(f64::from(snapshot.peak()))),
            "frames_rendered" => Some(IntrospectValue::Int(int_of_u64(snapshot.frames_rendered()))),
            "rejected" => Some(IntrospectValue::Int(int_of_u64(snapshot.rejected()))),
            "stolen" => Some(IntrospectValue::Int(int_of_u64(snapshot.stolen()))),
            // Join the lock-free per-voice numeric slots with the control-thread
            // label map (an atomic slot cannot hold the `String` label).
            "voices" => {
                let infos: Vec<RtVoiceInfo> = snapshot
                    .voices()
                    .iter()
                    .map(|v| RtVoiceInfo {
                        id: v.id,
                        label: controller.label_of(v.id).unwrap_or(""),
                        gain: v.authored_gain,
                        pan: v.authored_pan,
                        effective_gain: v.effective_gain,
                        effective_pan: v.effective_pan,
                        position_secs: v.position_secs,
                        looping: v.looping,
                        position: v.position,
                        distance: v.distance,
                    })
                    .collect();
                Some(IntrospectValue::json(&infos))
            }
            // Read the 3D listener / attenuation off the control-thread mirror
            // (the audio thread owns the engine and cannot be read lock-free,
            // but the control thread is their sole writer — see
            // [`AudioController::listener`]).
            "listener" => Some(IntrospectValue::json(&controller.listener())),
            "attenuation" => Some(IntrospectValue::json(&controller.attenuation())),
            // The full-pool policy, off the control-thread mirror — so an agent
            // watching `rejected`/`stolen` climb can discover which policy caused
            // it.
            "voice_policy" => Some(IntrospectValue::Text(
                controller.voice_policy().as_wire().to_string(),
            )),
            _ => None,
        }
    }

    /// The RT path exposes no writable slots: a schema-declared field is a
    /// read-only observation (`ReadOnly`), anything else is `UnknownPath`.
    /// Drive the audio thread through `invoke` instead.
    fn intervene(&mut self, path: &str, _value: IntrospectValue) -> Result<(), InterveneError> {
        Err(read_only_or_unknown(&self.schema(), path))
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            "play" => {
                let (name, opts) = parse_play(args)?;
                let clip = self
                    .clips
                    .get(&name)
                    .ok_or_else(|| unknown_clip("play", &name))?;
                // R1564 — pre-R1564 these two were the SAME value, under a
                // comment conceding it: "`Rejected` also covers a full command
                // ring — both are 'could not play now'". They are not the same
                // fact. An unknown clip will never play; a full ring is the one
                // an agent should retry. Now the wire says which.
                let id = self
                    .controller
                    .borrow_mut()
                    .play(clip, name.clone(), opts)
                    .ok_or_else(|| InvokeError::rejected(RING_FULL))?;
                self.pending_intents.push(Intent::new_static(
                    "audio.play",
                    IntrospectValue::Text(name),
                ));
                Ok(IntrospectValue::Int(int_of_u64(id)))
            }
            // The stop happens on the audio thread next render, and the control
            // thread cannot know synchronously whether a voice *matched* — so a
            // queued stop returns `Null` (not a misleading matched-bool). But a
            // stop that never queued (full ring) left the sound playing, so
            // that is surfaced as `Rejected`, not silently swallowed.
            "stop" => match args {
                IntrospectValue::Int(n) => {
                    queued_or_rejected(self.controller.borrow_mut().stop(u64_of(n)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            "stop_all" => queued_or_rejected(self.controller.borrow_mut().stop_all()),
            "set_master_gain" => match args {
                IntrospectValue::Float(g) => {
                    queued_or_rejected(self.controller.borrow_mut().set_master_gain(f32_of(g)))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // The per-voice params all take `{ "id": <int>, "<field>": … }`.
            "set_voice_gain" | "set_voice_pan" | "set_voice_position" => {
                self.invoke_voice_param(path, args)
            }
            // Move / re-orient the 3D listener from a partial `{position?,
            // forward?, up?}` object, patched against the control-thread mirror
            // (`query listener`). Full-spec or partial — omitted axes keep the
            // mirror's value.
            "set_listener" => match args {
                IntrospectValue::Json(v) => {
                    let listener = listener_from_partial(self.controller.borrow().listener(), &v);
                    queued_or_rejected(self.controller.borrow_mut().set_listener(listener))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // Tune the distance-attenuation model from a partial
            // `{reference_distance?, max_distance?, rolloff?}` object, patched
            // against the mirror (`query attenuation`).
            "set_attenuation" => match args {
                IntrospectValue::Json(v) => {
                    let attenuation =
                        attenuation_from_partial(self.controller.borrow().attenuation(), &v);
                    queued_or_rejected(self.controller.borrow_mut().set_attenuation(attenuation))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            // `"reject_newest" | "steal_oldest"` — choose the full-pool policy.
            "set_voice_policy" => match args {
                IntrospectValue::Text(s) => match VoicePolicy::from_wire(&s) {
                    Some(policy) => {
                        queued_or_rejected(self.controller.borrow_mut().set_voice_policy(policy))
                    }
                    // Type matches (Text) but the value is not a known policy.
                    None => Err(InvokeError::rejected(format!(
                        "set_voice_policy: {s:?} is not a voice policy \
                         (expected \"reject_newest\" or \"steal_oldest\")"
                    ))),
                },
                _ => Err(InvokeError::TypeMismatch),
            },
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

/// Build a full [`Listener`] from a partial `{position?, forward?, up?}` JSON
/// object, keeping `cur`'s value for any axis the object omits — the shared
/// partial-update semantics both audio Externals apply. The single-thread
/// [`AudioEngineExternal`] reads `cur` from its engine; the real-time
/// [`AudioControllerExternal`] reads it from the control-thread mirror
/// ([`AudioController::listener`]). A 2nd-consumer lift: the partial-update
/// *contract* (which fields are optional, what the fallback is) is one SSOT so
/// the two Externals cannot drift.
fn listener_from_partial(cur: Listener, v: &serde_json::Value) -> Listener {
    Listener::new(
        parse_vec3(v.get("position")).unwrap_or(cur.position),
        parse_vec3(v.get("forward")).unwrap_or(cur.forward),
        parse_vec3(v.get("up")).unwrap_or(cur.up),
    )
}

/// Build a full [`Attenuation`] from a partial `{reference_distance?,
/// max_distance?, rolloff?}` JSON object, keeping `cur`'s value for any field
/// the object omits — the attenuation twin of [`listener_from_partial`], shared
/// by both audio Externals.
fn attenuation_from_partial(cur: Attenuation, v: &serde_json::Value) -> Attenuation {
    Attenuation {
        reference_distance: parse_f32(v.get("reference_distance"))
            .unwrap_or(cur.reference_distance),
        max_distance: parse_f32(v.get("max_distance")).unwrap_or(cur.max_distance),
        rolloff: parse_f32(v.get("rolloff")).unwrap_or(cur.rolloff),
    }
}

/// Parse `{ "id": <int>, "position": [x,y,z] | null }` — the **wire form of an
/// emitter placement** (`null` un-spatialises the voice back to its authored pan).
///
/// Public for the same reason as [`parse_vec3`]: every consumer that accepts an
/// emitter over RPC — this crate's `set_voice_position`, and a binding that routes
/// emitters through [`crate::AudioWorld`] instead — must agree on the shape, and a
/// second hand-rolled parser is how a wire contract starts drifting.
#[must_use]
pub fn parse_emitter(v: &serde_json::Value) -> Option<(crate::engine::VoiceId, Option<Vec3>)> {
    let id = v.get("id").and_then(serde_json::Value::as_u64)?;
    let position = match v.get("position") {
        Some(serde_json::Value::Null) => None,
        p => Some(parse_vec3(p)?),
    };
    Some((id, position))
}

/// Parse an optional JSON `[x, y, z]` array into a [`Vec3`].
///
/// Public because it is the **wire form** of a world position: every consumer
/// that accepts a position over RPC — this crate's `set_voice_position` /
/// `set_listener`, and any binding that carries its own world pose across —
/// must agree on it, and a second hand-rolled parser is how a wire contract
/// starts drifting.
#[must_use]
pub fn parse_vec3(v: Option<&serde_json::Value>) -> Option<Vec3> {
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

/// Map a controller command's "was it queued?" flag to the RT invoke result:
/// `Null` on success, `Rejected` when the command ring was full. Every
/// *value-less* driving invoke routes through this so a full ring is *surfaced*
/// uniformly rather than silently dropped; `play` surfaces the same full-ring
/// `Rejected` directly (it returns the minted id, not `Null`). Either way a
/// command that never reached the audio thread did not take effect, and the
/// agent may retry.
fn queued_or_rejected(queued: bool) -> Result<IntrospectValue, InvokeError> {
    if queued {
        Ok(IntrospectValue::Null)
    } else {
        Err(InvokeError::rejected(RING_FULL))
    }
}

/// R1564 §5.15 — the one sentence for "the command never reached the audio
/// thread", shared by every value-less driving invoke and by `play`.
///
/// It says *retryable* explicitly, because that is the operator's next move and
/// it is the fact that distinguishes this refusal from an unknown clip — the
/// two were the same value before this round.
const RING_FULL: &str = "the audio command ring is full, so this command never \
                         reached the audio thread; retry after the next render";

/// R1564 §5.15 — "no clip by that name", naming the name and the surface's own
/// vocabulary. The clip set is small and enumerable, so a refusal that lists it
/// turns a typo into a one-look fix.
fn unknown_clip(action: &str, name: &str) -> InvokeError {
    InvokeError::rejected(format!("{action}: no clip named {name:?} on this surface"))
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
    use pinion_core::test_fixtures::assert_refused_saying;

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
        assert_refused_saying(
            &ext.invoke("play", IntrospectValue::Text("nope".to_string())),
            "no clip named \"nope\"",
        );
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
        assert_refused_saying(
            &ext.invoke("send", IntrospectValue::Text("nope".to_string())),
            "no clip named \"nope\"",
        );
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
    fn intervene_of_a_read_only_field_is_read_only_not_unknown() {
        let mut ext = director();
        // `voice_count` is a declared, query-only field → `ReadOnly`, never
        // `UnknownPath` (which would deny the schema declares it).
        assert!(matches!(
            ext.intervene("voice_count", IntrospectValue::Int(3)),
            Err(InterveneError::ReadOnly)
        ));
        // A path the schema does not declare is genuinely unknown.
        assert!(matches!(
            ext.intervene("nonexistent", IntrospectValue::Int(3)),
            Err(InterveneError::UnknownPath)
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
        // The pool bound is introspectable before any render (fixed at creation).
        assert!(matches!(
            ext.query("max_voices"),
            Some(IntrospectValue::Int(8))
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
    fn rt_stop_by_id_is_fire_and_forget() {
        let (mut ext, mut renderer) = rt_director(8);
        let id = match ext.invoke("play", IntrospectValue::Text("bell".to_string())) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected id, got {other:?}"),
        };
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        // Fire-and-forget: `Null`, not a misleading bool (async stop).
        assert!(matches!(
            ext.invoke("stop", IntrospectValue::Int(id)),
            Ok(IntrospectValue::Null)
        ));
        renderer.render(&mut out);
        assert!(matches!(
            ext.query("voice_count"),
            Some(IntrospectValue::Int(0))
        ));
    }

    #[test]
    fn rt_invoke_param_drive_reaches_the_mix() {
        let (mut ext, mut renderer) = rt_director(8);
        // Centred full-scale tone → constant-power centre ~0.707/leg.
        ext.invoke("play", IntrospectValue::Text("bell".to_string()))
            .expect("play");
        // Duck the whole mix to half, and the one voice to 0.5 gain.
        ext.invoke("set_master_gain", IntrospectValue::Float(0.5))
            .expect("master gain");
        let id = 1; // control-thread-minted first id
        ext.invoke(
            "set_voice_gain",
            IntrospectValue::Json(serde_json::json!({ "id": id, "gain": 0.5 })),
        )
        .expect("voice gain");

        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        // centre 0.707 × voice-gain 0.5 × master 0.5 ≈ 0.177.
        match ext.query("peak") {
            Some(IntrospectValue::Float(p)) => assert!(
                (p - f64::from(std::f32::consts::FRAC_1_SQRT_2) * 0.25).abs() < 1e-2,
                "master + voice gain both applied: peak {p}"
            ),
            other => panic!("expected peak float, got {other:?}"),
        }
        // A malformed set_voice_gain object is a type error, not a silent no-op.
        assert!(matches!(
            ext.invoke(
                "set_voice_gain",
                IntrospectValue::Json(serde_json::json!({ "id": 1 }))
            ),
            Err(InvokeError::TypeMismatch)
        ));
    }

    #[test]
    fn rt_reclaim_frees_returned_voices_on_the_control_thread() {
        let (mut ext, mut renderer) = rt_director(8);
        // A short one-shot that finishes and is returned over the queue.
        let blip = AudioClip::new(48_000, 1, vec![1.0; 2]).shared();
        ext = ext.with_clip("blip", blip);
        ext.invoke("play", IntrospectValue::Text("blip".to_string()))
            .expect("play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out); // one-shot finishes, returned to the queue
        // A query does not reclaim; the explicit delegate does.
        assert_eq!(ext.reclaim(), 1, "the retired voice is freed here");
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
        assert_refused_saying(
            &ext.invoke("play", IntrospectValue::Text("nope".to_string())),
            "no clip named \"nope\"",
        );
        assert!(matches!(
            ext.invoke("bogus", IntrospectValue::Null),
            Err(InvokeError::UnknownPath)
        ));
        // A declared field is read-only (not "unknown"); an undeclared path is
        // unknown; an unknown query is None.
        assert!(matches!(
            ext.intervene("voice_count", IntrospectValue::Float(1.0)),
            Err(InterveneError::ReadOnly)
        ));
        assert!(matches!(
            ext.intervene("nonexistent", IntrospectValue::Float(1.0)),
            Err(InterveneError::UnknownPath)
        ));
        assert!(ext.query("nonexistent").is_none());
    }

    #[test]
    fn rt_listener_query_reads_mirror_and_invoke_drives_the_mix() {
        let (mut ext, mut renderer) = rt_director(8);
        // The default listener is readable off the control-thread mirror.
        match ext.query("listener") {
            Some(IntrospectValue::Json(v)) => {
                assert_eq!(v["position"], serde_json::json!([0.0, 0.0, 0.0]));
                assert_eq!(v["forward"], serde_json::json!([0.0, 0.0, -1.0]));
            }
            other => panic!("expected listener json, got {other:?}"),
        }

        // Play the bell 4 units to the world-right → distance 4, default rolloff
        // gives gain 1/(1+3) = 0.25.
        ext.invoke(
            "play",
            IntrospectValue::Json(serde_json::json!({
                "name": "bell", "position": [4.0, 0.0, 0.0]
            })),
        )
        .expect("spatial play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        let far = match ext.query("peak") {
            Some(IntrospectValue::Float(p)) => p,
            other => panic!("expected peak, got {other:?}"),
        };
        assert!((far - 0.25).abs() < 1e-2, "distant voice attenuated: {far}");

        // Move the listener onto [3,0,0] — a partial update: only `position` is
        // given, so forward/up keep the mirror's value. Distance → 1 → full gain.
        ext.invoke(
            "set_listener",
            IntrospectValue::Json(serde_json::json!({ "position": [3.0, 0.0, 0.0] })),
        )
        .expect("move listener");
        // The mirror advanced and kept the unspecified axes.
        match ext.query("listener") {
            Some(IntrospectValue::Json(v)) => {
                assert_eq!(v["position"], serde_json::json!([3.0, 0.0, 0.0]));
                assert_eq!(
                    v["forward"],
                    serde_json::json!([0.0, 0.0, -1.0]),
                    "forward kept"
                );
            }
            other => panic!("expected listener json, got {other:?}"),
        }
        renderer.render(&mut out);
        let near = match ext.query("peak") {
            Some(IntrospectValue::Float(p)) => p,
            other => panic!("expected peak, got {other:?}"),
        };
        assert!(
            near > far + 0.5,
            "the listener move reached the mix: {far} -> {near}"
        );
        assert!(
            (near - 1.0).abs() < 1e-2,
            "on top of the source → full gain: {near}"
        );
    }

    #[test]
    fn rt_set_attenuation_changes_the_falloff() {
        let (mut ext, mut renderer) = rt_director(8);
        ext.invoke(
            "play",
            IntrospectValue::Json(serde_json::json!({
                "name": "bell", "position": [4.0, 0.0, 0.0]
            })),
        )
        .expect("spatial play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        // Zero the rolloff → no distance falloff, so the distance-4 voice is
        // full gain. A partial update: reference/max keep the mirror's value.
        ext.invoke(
            "set_attenuation",
            IntrospectValue::Json(serde_json::json!({ "rolloff": 0.0 })),
        )
        .expect("set attenuation");
        match ext.query("attenuation") {
            Some(IntrospectValue::Json(v)) => {
                assert!(v["rolloff"].as_f64().unwrap().abs() < 1e-9);
                assert!(
                    (v["reference_distance"].as_f64().unwrap() - 1.0).abs() < 1e-6,
                    "reference kept"
                );
            }
            other => panic!("expected attenuation json, got {other:?}"),
        }
        renderer.render(&mut out);
        match ext.query("peak") {
            Some(IntrospectValue::Float(p)) => {
                assert!(
                    (p - 1.0).abs() < 1e-2,
                    "zero rolloff → full gain at distance 4: {p}"
                );
            }
            other => panic!("expected peak, got {other:?}"),
        }
    }

    #[test]
    fn rt_listener_attenuation_are_query_read_invoke_write_not_intervene() {
        let (mut ext, _renderer) = rt_director(8);
        // Declared (query-readable) but driven via invoke → an intervene of them
        // is `ReadOnly`, pointing the caller at `invoke set_listener`.
        assert!(matches!(
            ext.intervene(
                "listener",
                IntrospectValue::Json(serde_json::json!({ "position": [1.0, 0.0, 0.0] }))
            ),
            Err(InterveneError::ReadOnly)
        ));
        assert!(matches!(
            ext.intervene(
                "attenuation",
                IntrospectValue::Json(serde_json::json!({ "rolloff": 0.0 }))
            ),
            Err(InterveneError::ReadOnly)
        ));
        // A wrong invoke arg type is a type error, not a silent no-op.
        assert!(matches!(
            ext.invoke("set_listener", IntrospectValue::Float(1.0)),
            Err(InvokeError::TypeMismatch)
        ));
        assert!(matches!(
            ext.invoke("set_attenuation", IntrospectValue::Int(1)),
            Err(InvokeError::TypeMismatch)
        ));
    }

    #[test]
    fn rt_param_drive_surfaces_a_full_ring_as_rejected() {
        // Command ring capacity 1: a stop_all fills it, then every driving
        // invoke is refused *loudly* (`Rejected`), never a silent success.
        let (controller, mut renderer) =
            crate::rt::realtime_channel(AudioEngine::new(48_000), 1, 8);
        let mut ext = AudioControllerExternal::new(controller)
            .with_clip("bell", AudioClip::new(48_000, 1, vec![1.0; 4096]).shared());
        ext.invoke("stop_all", IntrospectValue::Null)
            .expect("stop_all fills the 1-deep ring");
        for drive in [
            ext.invoke("set_master_gain", IntrospectValue::Float(0.5)),
            ext.invoke(
                "set_listener",
                IntrospectValue::Json(serde_json::json!({ "position": [1.0, 0.0, 0.0] })),
            ),
            ext.invoke(
                "set_attenuation",
                IntrospectValue::Json(serde_json::json!({ "rolloff": 0.0 })),
            ),
            ext.invoke("stop", IntrospectValue::Int(1)),
        ] {
            // R1564 — a full ring now SAYS it is a full ring, and says the
            // command never reached the audio thread. Before this round it was
            // the same value an unknown clip produced.
            assert_refused_saying(&drive, "audio command ring is full");
        }
        // The refused set_listener did not advance the mirror (no drift).
        match ext.query("listener") {
            Some(IntrospectValue::Json(v)) => {
                assert_eq!(
                    v["position"],
                    serde_json::json!([0.0, 0.0, 0.0]),
                    "mirror unchanged"
                );
            }
            other => panic!("expected listener json, got {other:?}"),
        }
        // Drain the ring; the drive succeeds now.
        let mut out = [0.0f32; 8];
        renderer.render(&mut out);
        assert!(matches!(
            ext.invoke("set_master_gain", IntrospectValue::Float(0.5)),
            Ok(IntrospectValue::Null)
        ));
    }

    #[test]
    fn rt_voices_query_reads_per_voice_detail_with_labels() {
        let (mut ext, mut renderer) = rt_director(8);
        ext = ext.with_clip("waves", AudioClip::new(48_000, 1, vec![0.5; 4096]).shared());
        ext.invoke("play", IntrospectValue::Text("bell".to_string()))
            .expect("flat play");
        // A spatial voice 4 units to the right → effective gain 1.0 × 0.25.
        ext.invoke(
            "play",
            IntrospectValue::Json(serde_json::json!({
                "name": "waves", "position": [4.0, 0.0, 0.0]
            })),
        )
        .expect("spatial play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);

        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert_eq!(items.len(), 2, "both live voices listed");
                let bell = items
                    .iter()
                    .find(|v| v["label"] == "bell")
                    .expect("bell labelled from the control-thread map");
                assert!((bell["effective_gain"].as_f64().unwrap() - 1.0).abs() < 1e-3);
                assert!(bell.get("position").is_none(), "flat voice omits position");
                let waves = items
                    .iter()
                    .find(|v| v["label"] == "waves")
                    .expect("waves labelled");
                assert_eq!(waves["position"], serde_json::json!([4.0, 0.0, 0.0]));
                assert!((waves["distance"].as_f64().unwrap() - 4.0).abs() < 1e-3);
                // Authored gain stays 1.0; effective = 1.0 x 0.25 distance atten
                // — both readable, so a debugger can tell "low gain" from "far".
                assert!(
                    (waves["gain"].as_f64().unwrap() - 1.0).abs() < 1e-3,
                    "authored gain published"
                );
                assert!(
                    (waves["effective_gain"].as_f64().unwrap() - 0.25).abs() < 1e-3,
                    "effective gain = 1.0 voice gain x 0.25 distance atten"
                );
                assert!((waves["effective_pan"].as_f64().unwrap() - 1.0).abs() < 1e-3);
            }
            other => panic!("expected voices array, got {other:?}"),
        }
    }

    #[test]
    fn rt_voices_empties_and_label_prunes_after_a_one_shot_finishes() {
        let (mut ext, mut renderer) = rt_director(8);
        ext = ext.with_clip("blip", AudioClip::new(48_000, 1, vec![1.0; 2]).shared());
        ext.invoke("play", IntrospectValue::Text("blip".to_string()))
            .expect("play");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out); // the 2-frame one-shot finishes this block
        // The finished voice is gone from the per-voice read (reaped before the
        // publish).
        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert!(items.is_empty(), "finished voice not listed");
            }
            other => panic!("expected empty voices array, got {other:?}"),
        }
        // Reclaim frees the returned voice and prunes its label on the control
        // thread.
        assert_eq!(ext.reclaim(), 1, "the retired voice is reclaimed");
    }

    #[test]
    fn rt_voices_is_read_only_via_intervene() {
        let (mut ext, _renderer) = rt_director(8);
        // Declared (query-readable) but not an intervene slot.
        assert!(matches!(
            ext.intervene("voices", IntrospectValue::Json(serde_json::json!([]))),
            Err(InterveneError::ReadOnly)
        ));
    }

    #[test]
    fn rt_stolen_metric_surfaces_voice_stealing() {
        use crate::engine::VoicePolicy;
        // A StealOldest engine, pool of 2: a third play is admitted by stealing,
        // observable as the `stolen` metric over RPC (and `rejected` stays 0).
        let mut engine = AudioEngine::new(48_000);
        engine.set_voice_policy(VoicePolicy::StealOldest);
        let (controller, mut renderer) = crate::rt::realtime_channel(engine, 16, 2);
        let mut ext = AudioControllerExternal::new(controller)
            .with_clip("bell", AudioClip::new(48_000, 1, vec![1.0; 4096]).shared());
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
            matches!(ext.query("stolen"), Some(IntrospectValue::Int(1))),
            "the steal is introspectable"
        );
        assert!(
            matches!(ext.query("rejected"), Some(IntrospectValue::Int(0))),
            "no rejection under StealOldest"
        );
    }

    #[test]
    fn rt_set_voice_position_moves_an_emitter_over_rpc() {
        let (mut ext, mut renderer) = rt_director(8);
        let id = match ext.invoke(
            "play",
            IntrospectValue::Json(
                serde_json::json!({ "name": "bell", "position": [4.0, 0.0, 0.0] }),
            ),
        ) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected id, got {other:?}"),
        };
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        let dist = |ext: &AudioControllerExternal| -> f64 {
            match ext.query("voices") {
                Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                    items[0]["distance"].as_f64().expect("3D distance")
                }
                other => panic!("expected voices, got {other:?}"),
            }
        };
        assert!(
            (dist(&ext) - 4.0).abs() < 1e-3,
            "emitter starts at distance 4"
        );

        // Move the emitter onto the listener over RPC → distance 1.
        ext.invoke(
            "set_voice_position",
            IntrospectValue::Json(serde_json::json!({ "id": id, "position": [1.0, 0.0, 0.0] })),
        )
        .expect("move emitter");
        renderer.render(&mut out);
        assert!(
            (dist(&ext) - 1.0).abs() < 1e-3,
            "the emitter moved on the audio thread over the RT wire"
        );

        // `null` un-spatialises back to a flat voice.
        ext.invoke(
            "set_voice_position",
            IntrospectValue::Json(serde_json::json!({ "id": id, "position": null })),
        )
        .expect("un-spatialise");
        renderer.render(&mut out);
        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert!(
                    items[0].get("distance").is_none(),
                    "un-spatialised: flat again"
                );
            }
            other => panic!("expected voices, got {other:?}"),
        }
    }

    #[test]
    fn rt_set_voice_pan_reaches_the_mix_over_rpc() {
        let (mut ext, mut renderer) = rt_director(8);
        let id = match ext.invoke("play", IntrospectValue::Text("bell".to_string())) {
            Ok(IntrospectValue::Int(id)) => id,
            other => panic!("expected id, got {other:?}"),
        };
        ext.invoke(
            "set_voice_pan",
            IntrospectValue::Json(serde_json::json!({ "id": id, "pan": 1.0 })),
        )
        .expect("set pan");
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        match ext.query("voices") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert!(
                    (items[0]["effective_pan"].as_f64().unwrap() - 1.0).abs() < 1e-3,
                    "the pan reached the mix (flat voice → effective == authored)"
                );
            }
            other => panic!("expected voices, got {other:?}"),
        }
    }

    #[test]
    fn rt_voice_policy_query_invoke_and_read_only() {
        // Pool of 2; default RejectNewest. Read it, switch to StealOldest over
        // RPC, and prove the audio thread's behaviour actually changed.
        let (mut ext, mut renderer) = rt_director(2);
        assert!(matches!(
            ext.query("voice_policy"),
            Some(IntrospectValue::Text(ref s)) if s == "reject_newest"
        ));
        ext.invoke(
            "set_voice_policy",
            IntrospectValue::Text("steal_oldest".to_string()),
        )
        .expect("set policy");
        assert!(matches!(
            ext.query("voice_policy"),
            Some(IntrospectValue::Text(ref s)) if s == "steal_oldest"
        ));
        // The queued policy applies before the queued plays, so the third steals.
        for _ in 0..3 {
            ext.invoke("play", IntrospectValue::Text("bell".to_string()))
                .expect("play");
        }
        let mut out = [0.0f32; 256];
        renderer.render(&mut out);
        assert!(
            matches!(ext.query("stolen"), Some(IntrospectValue::Int(1))),
            "the RPC policy change reached the audio thread"
        );
        assert!(matches!(
            ext.query("rejected"),
            Some(IntrospectValue::Int(0))
        ));
        // An unknown policy string is Rejected (Text type ok, value invalid).
        assert_refused_saying(
            &ext.invoke(
                "set_voice_policy",
                IntrospectValue::Text("nonsense".to_string()),
            ),
            "\"nonsense\" is not a voice policy",
        );
        // voice_policy is query-read, invoke-write → intervene is ReadOnly.
        assert!(matches!(
            ext.intervene(
                "voice_policy",
                IntrospectValue::Text("steal_oldest".to_string())
            ),
            Err(InterveneError::ReadOnly)
        ));
    }
}
