//! The AI-first drive surface: a [`NarrativeExternal`] wrapping the read
//! model.
//!
//! This is the §5.15 `External` escape hatch used the way §2 #2 / #7
//! intend — not to embed an opaque subsystem, but to expose the narrative
//! read-model over the RPC introspection triad so an AI agent drives the
//! whole walk with `scene/query` + `scene/invoke` and reads it back as
//! data:
//!
//! - **query** — `telling` / `world` / `scene` / `world_count` /
//!   `scene_count` / `branch_id` / `title` / `intent` / `disclosure_count`
//!   / `disclosures` / `world_ids` / `fork_tree`.
//! - **intervene** — set `world` / `scene` to jump the cursor (clamped).
//! - **invoke** — semantic verbs `next_scene` / `prev_scene` /
//!   `next_world` / `prev_world` / `goto`, plus the `send` wire the
//!   keyboard `keybinding → forward` path arrives on.
//!
//! Every cursor move emits a `narrative.moved` intent so the shell knows
//! the visible state changed and an RPC observer can watch the walk.

use std::rc::Rc;

use pinion_core::external::{
    ExternalIntrospect, InterveneError, IntrospectSchema, IntrospectValue, InvokeError, int_of,
};
use pinion_core::intent::Intent;

use crate::state::{NarrativeCursor, NarrativeState};

/// The `narrative.moved` intent tag emitted on every cursor transition.
pub const MOVED_INTENT: &str = "narrative.moved";

/// `External` adapter over a shared [`NarrativeState`].
#[derive(Debug)]
pub struct NarrativeExternal {
    state: Rc<NarrativeState>,
    pending_intents: Vec<Intent>,
}

impl NarrativeExternal {
    /// Wrap a shared read-model. The `Rc` is the same one the view reads,
    /// so a navigation here repaints there.
    #[must_use]
    pub fn new(state: Rc<NarrativeState>) -> Self {
        Self {
            state,
            pending_intents: Vec::new(),
        }
    }

    /// Apply a navigation verb, emitting `narrative.moved` on an actual
    /// move, and return the resulting cursor as JSON.
    fn apply_nav(&mut self, verb: &str) -> Result<IntrospectValue, InvokeError> {
        let moved = match verb {
            "next_scene" => self.state.next_scene(),
            "prev_scene" => self.state.prev_scene(),
            "next_world" => self.state.next_world(),
            "prev_world" => self.state.prev_world(),
            _ => return Err(InvokeError::UnknownPath),
        };
        if moved {
            self.emit_moved();
        }
        Ok(cursor_json(self.state.cursor()))
    }

    fn emit_moved(&mut self) {
        self.pending_intents.push(Intent::new_static(
            MOVED_INTENT,
            cursor_json(self.state.cursor()),
        ));
    }
}

// This node paints nothing (the binding's `view` is the paint scene) and
// emits §5.20 intents — the RPC-only read-write introspection skeleton.
pinion_core::intent_query_external_impl!(NarrativeExternal);

impl ExternalIntrospect for NarrativeExternal {
    fn schema(&self) -> IntrospectSchema {
        IntrospectSchema::new(&[
            ("telling", "text"),
            ("world", "int"),
            ("scene", "int"),
            ("world_count", "int"),
            ("scene_count", "int"),
            ("branch_id", "text"),
            ("title", "text"),
            ("intent", "text"),
            ("disclosure_count", "int"),
            ("disclosures", "json"),
            ("world_ids", "json"),
            ("fork_tree", "json"),
        ])
    }

    fn query(&self, path: &str) -> Option<IntrospectValue> {
        let cursor = self.state.cursor();
        match path {
            "telling" => Some(IntrospectValue::Text(self.state.world().telling.clone())),
            "world" => Some(IntrospectValue::Int(i64::from(cursor.world))),
            "scene" => Some(IntrospectValue::Int(i64::from(cursor.scene))),
            "world_count" => Some(IntrospectValue::Int(int_of(self.state.world_count()))),
            "scene_count" => Some(IntrospectValue::Int(int_of(self.state.scene_count()))),
            "branch_id" => Some(IntrospectValue::Text(
                self.state
                    .current_world_line()
                    .map(|w| w.branch_id.clone())
                    .unwrap_or_default(),
            )),
            "title" => Some(IntrospectValue::Text(
                self.state
                    .current_scene()
                    .map(|s| s.title.clone())
                    .unwrap_or_default(),
            )),
            "intent" => Some(IntrospectValue::Text(
                self.state
                    .current_scene()
                    .map(|s| s.intent.clone())
                    .unwrap_or_default(),
            )),
            "disclosure_count" => Some(IntrospectValue::Int(int_of(
                self.state
                    .current_scene()
                    .map_or(0, |s| s.disclosures.len()),
            ))),
            "disclosures" => Some(
                self.state
                    .current_scene()
                    .map_or(IntrospectValue::Null, |s| {
                        IntrospectValue::json(&s.disclosures)
                    }),
            ),
            "world_ids" => {
                let ids: Vec<&str> = self
                    .state
                    .world()
                    .worlds
                    .iter()
                    .map(|w| w.branch_id.as_str())
                    .collect();
                Some(IntrospectValue::json(&ids))
            }
            "fork_tree" => Some(IntrospectValue::json(&self.state.world().fork_tree)),
            _ => None,
        }
    }

    fn intervene(&mut self, path: &str, value: IntrospectValue) -> Result<(), InterveneError> {
        let cursor = self.state.cursor();
        let moved = match (path, value) {
            ("world", IntrospectValue::Int(n)) => self.state.goto(u16_of_i64(n), 0),
            ("scene", IntrospectValue::Int(n)) => self.state.goto(cursor.world, u16_of_i64(n)),
            ("world" | "scene", _) => return Err(InterveneError::TypeMismatch),
            _ => return Err(InterveneError::UnknownPath),
        };
        if moved {
            self.emit_moved();
        }
        Ok(())
    }

    fn invoke(
        &mut self,
        path: &str,
        args: IntrospectValue,
    ) -> Result<IntrospectValue, InvokeError> {
        match path {
            // The keyboard `keybinding → forward → send_to_primary` path
            // arrives here carrying the event name as the Text arg.
            "send" => match args {
                IntrospectValue::Text(verb) => self.apply_nav(&verb),
                _ => Err(InvokeError::TypeMismatch),
            },
            // Semantic verbs an AI agent / RPC client invokes directly.
            "next_scene" | "prev_scene" | "next_world" | "prev_world" => self.apply_nav(path),
            "goto" => match args {
                IntrospectValue::Json(v) => {
                    let world = v.get("world").and_then(serde_json::Value::as_u64);
                    let scene = v.get("scene").and_then(serde_json::Value::as_u64);
                    let moved = self.state.goto(
                        u16_of_u64(world.unwrap_or(0)),
                        u16_of_u64(scene.unwrap_or(0)),
                    );
                    if moved {
                        self.emit_moved();
                    }
                    Ok(cursor_json(self.state.cursor()))
                }
                _ => Err(InvokeError::TypeMismatch),
            },
            _ => Err(InvokeError::UnknownPath),
        }
    }
}

fn cursor_json(cursor: NarrativeCursor) -> IntrospectValue {
    IntrospectValue::json(&cursor)
}

fn u16_of_i64(n: i64) -> u16 {
    u16::try_from(n.clamp(0, i64::from(u16::MAX))).unwrap_or(0)
}

fn u16_of_u64(n: u64) -> u16 {
    u16::try_from(n.min(u64::from(u16::MAX))).unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Disclosure, PlayableWorld, SceneNode, WorldLine};
    use pinion_core::external::External;
    use pinion_core::reactive::Owner;

    fn sample() -> PlayableWorld {
        PlayableWorld {
            telling: "reader".to_string(),
            worlds: vec![
                WorldLine {
                    branch_id: "main".to_string(),
                    scenes: vec![
                        SceneNode {
                            idx: 0,
                            title: "물때표".to_string(),
                            intent: "규칙 각인".to_string(),
                            disclosures: vec![Disclosure {
                                mode: "plant".to_string(),
                                fact: "만조엔 길이 사라진다".to_string(),
                                first_at: "ch01".to_string(),
                            }],
                        },
                        SceneNode {
                            idx: 1,
                            title: "밤의 부름".to_string(),
                            ..SceneNode::default()
                        },
                    ],
                },
                WorldLine {
                    branch_id: "ending-flee".to_string(),
                    scenes: vec![SceneNode {
                        idx: 0,
                        title: "떠나는 발".to_string(),
                        ..SceneNode::default()
                    }],
                },
            ],
            ..PlayableWorld::default()
        }
    }

    fn with_external<R>(body: impl FnOnce(&mut NarrativeExternal) -> R) -> R {
        let owner = Owner::new();
        owner.run(|| {
            let state = Rc::new(NarrativeState::new(sample()));
            let mut ext = NarrativeExternal::new(state);
            body(&mut ext)
        })
    }

    fn text(v: Option<IntrospectValue>) -> String {
        match v {
            Some(IntrospectValue::Text(s)) => s,
            other => panic!("expected Text, got {other:?}"),
        }
    }

    fn int(v: Option<IntrospectValue>) -> i64 {
        match v {
            Some(IntrospectValue::Int(n)) => n,
            other => panic!("expected Int, got {other:?}"),
        }
    }

    #[test]
    fn query_projects_current_scene() {
        with_external(|ext| {
            assert_eq!(text(ext.query("telling")), "reader");
            assert_eq!(text(ext.query("branch_id")), "main");
            assert_eq!(text(ext.query("title")), "물때표");
            assert_eq!(int(ext.query("scene_count")), 2);
            assert_eq!(int(ext.query("world_count")), 2);
            assert_eq!(int(ext.query("disclosure_count")), 1);
            assert!(ext.query("unknown_path").is_none());
        });
    }

    #[test]
    fn invoke_next_scene_moves_and_emits_intent() {
        with_external(|ext| {
            assert!(!ext.is_dirty());
            ext.invoke("next_scene", IntrospectValue::Null)
                .expect("next_scene invokes");
            assert_eq!(int(ext.query("scene")), 1);
            assert_eq!(text(ext.query("title")), "밤의 부름");
            assert!(ext.is_dirty(), "a move queues an intent");

            let mut drained = Vec::new();
            ext.drain_intents(&mut |i| drained.push(i));
            assert_eq!(drained.len(), 1);
            assert_eq!(drained[0].tag, MOVED_INTENT);
            assert!(!ext.is_dirty(), "drained");
        });
    }

    #[test]
    fn send_wire_drives_navigation() {
        with_external(|ext| {
            // The keyboard path: invoke("send", <event name>).
            ext.invoke("send", IntrospectValue::Text("next_scene".to_string()))
                .expect("send routes");
            assert_eq!(int(ext.query("scene")), 1);
        });
    }

    #[test]
    fn intervene_sets_cursor_clamped() {
        with_external(|ext| {
            ext.intervene("world", IntrospectValue::Int(9))
                .expect("world intervene");
            assert_eq!(int(ext.query("world")), 1, "clamped to last world");
            assert_eq!(text(ext.query("branch_id")), "ending-flee");
            assert!(matches!(
                ext.intervene("world", IntrospectValue::Text("x".to_string())),
                Err(InterveneError::TypeMismatch)
            ));
            assert!(matches!(
                ext.intervene("nope", IntrospectValue::Int(0)),
                Err(InterveneError::UnknownPath)
            ));
        });
    }

    #[test]
    fn goto_via_json_moves_cursor() {
        with_external(|ext| {
            let args = IntrospectValue::Json(serde_json::json!({ "world": 1, "scene": 0 }));
            ext.invoke("goto", args).expect("goto invokes");
            assert_eq!(int(ext.query("world")), 1);
        });
    }

    #[test]
    fn disclosures_query_is_json_array() {
        with_external(|ext| match ext.query("disclosures") {
            Some(IntrospectValue::Json(serde_json::Value::Array(items))) => {
                assert_eq!(items.len(), 1);
                assert_eq!(items[0]["mode"], "plant");
            }
            other => panic!("expected Json array, got {other:?}"),
        });
    }
}
