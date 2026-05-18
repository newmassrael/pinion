//! R51.73 §5.40 — `focus/set` + `focus/get` RPC methods.
//!
//! The §5.40 accessibility axis ships two parallel control channels:
//! AccessKit (Windows UIA / macOS AX / Linux AT-SPI / Android) for
//! screen-reader clients, and the JSON-RPC introspect path (§5.7,
//! §2 invariant #2 "RPC headless as AI primary path") for AI
//! clients. The AT path already exposes
//! [`accesskit::Action::Focus`]; this module is the RPC dual so AI
//! clients can issue the same operation programmatically without
//! synthesising a winit keyboard event.
//!
//! ## Method shapes
//!
//! - `focus/set` — params: `{ "tag": "<widget_tag>" | null }`.
//!   `null` clears focus; a string sets focus to that tag. The tag
//!   must appear in the shell's current
//!   [`pinion_runtime::FocusManager::tab_order`] enumeration
//!   (otherwise the method errors with `tag_not_focusable`). On
//!   success returns `{ "focused": "<tag>" | null }` mirroring the
//!   new state.
//!
//! - `focus/get` — no params. Returns `{ "focused": "<tag>" | null,
//!   "tab_order": ["<tag>", ...] }` so AI clients can discover what
//!   the application considers focusable without scraping the paint
//!   scene.

use pinion_runtime::FocusManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Reasons [`focus_set`] can fail.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusError {
    /// The shell did not register a focus manager in the
    /// [`crate::DispatchContext`] — `focus/*` methods are unavailable
    /// for this dispatch. The application embedding pinion-rpc may
    /// have opted out of focus surfacing.
    Unavailable,
    /// The supplied tag is not in the current `tab_order`. AI clients
    /// should call `focus/get` first to discover the focusable set.
    NotFocusable(String),
}

/// Outcome of a [`focus_set`] / [`focus_get`] call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FocusState {
    /// Currently focused widget tag, if any.
    pub focused: Option<String>,
    /// Optional tab-order enumeration. Populated by [`focus_get`]
    /// (so the AI client sees the focusable set in one round trip);
    /// `None` from [`focus_set`] (the caller already knows the tag
    /// it just set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab_order: Option<Vec<String>>,
}

/// `focus/set` params payload. `tag = None` clears focus.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct FocusSetParams {
    /// Widget tag to focus, or `null` to clear.
    pub tag: Option<String>,
}

/// Set or clear focus. See module docs for shape.
///
/// # Errors
///
/// - [`FocusError::Unavailable`] — no focus manager on the context.
/// - [`FocusError::NotFocusable`] — `tag` is not in the current
///   `tab_order`. (Clearing focus, `tag = None`, never errors.)
pub fn focus_set(
    focus: &mut FocusManager,
    params: &FocusSetParams,
) -> Result<FocusState, FocusError> {
    match params.tag.as_deref() {
        None => {
            let _ = focus.focus_clear();
        }
        Some(tag) => {
            // `focus_set` returns `false` when the tag is absent
            // from `tab_order` OR already focused. Distinguish the
            // two by checking `tab_order` membership explicitly so
            // the no-op (already-focused) case returns success.
            let in_order = focus.tab_order().iter().any(|t| t == tag);
            if !in_order {
                return Err(FocusError::NotFocusable(tag.to_owned()));
            }
            let _ = focus.focus_set(tag);
        }
    }
    Ok(FocusState {
        focused: focus.focused().map(str::to_owned),
        tab_order: None,
    })
}

/// `focus/get`. Returns the current focus + tab order in one round
/// trip.
pub fn focus_get(focus: &FocusManager) -> FocusState {
    FocusState {
        focused: focus.focused().map(str::to_owned),
        tab_order: Some(focus.tab_order().to_vec()),
    }
}

/// JSON-RPC adapter for `focus/set`. Parses the `params` Value into
/// [`FocusSetParams`], runs [`focus_set`], and lifts the result into
/// the dispatcher's `(serde_json::Value, RpcError)` shape.
pub(crate) fn handle_focus_set(
    focus: Option<&mut FocusManager>,
    params: Option<&Value>,
) -> Result<Value, crate::dispatch::RpcError> {
    let Some(focus) = focus else {
        return Err(crate::dispatch::RpcError {
            code: -32004,
            message: "focus manager unavailable".to_string(),
            data: None,
        });
    };
    let parsed: FocusSetParams = match params {
        Some(p) => serde_json::from_value(p.clone()).map_err(|e| crate::dispatch::RpcError {
            code: -32602,
            message: "Invalid params".to_string(),
            data: Some(Value::String(e.to_string())),
        })?,
        None => FocusSetParams { tag: None },
    };
    let state = focus_set(focus, &parsed).map_err(|e| match e {
        FocusError::Unavailable => crate::dispatch::RpcError {
            code: -32004,
            message: "focus manager unavailable".to_string(),
            data: None,
        },
        FocusError::NotFocusable(tag) => crate::dispatch::RpcError {
            code: -32602,
            message: "tag_not_focusable".to_string(),
            data: Some(Value::String(tag)),
        },
    })?;
    serde_json::to_value(state).map_err(|e| crate::dispatch::RpcError {
        code: -32603,
        message: "Internal error".to_string(),
        data: Some(Value::String(e.to_string())),
    })
}

/// JSON-RPC adapter for `focus/get`.
pub(crate) fn handle_focus_get(
    focus: Option<&FocusManager>,
) -> Result<Value, crate::dispatch::RpcError> {
    let Some(focus) = focus else {
        return Err(crate::dispatch::RpcError {
            code: -32004,
            message: "focus manager unavailable".to_string(),
            data: None,
        });
    };
    let state = focus_get(focus);
    serde_json::to_value(state).map_err(|e| crate::dispatch::RpcError {
        code: -32603,
        message: "Internal error".to_string(),
        data: Some(Value::String(e.to_string())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fm_with(tags: &[&str]) -> FocusManager {
        let mut fm = FocusManager::new();
        fm.update_focusable_tags(tags.iter().map(|t| (*t).to_owned()).collect());
        fm
    }

    #[test]
    fn set_focus_to_known_tag_succeeds() {
        let mut fm = fm_with(&["main_btn", "main_cb"]);
        let out = focus_set(
            &mut fm,
            &FocusSetParams { tag: Some("main_btn".to_owned()) },
        )
        .expect("known tag");
        assert_eq!(out.focused.as_deref(), Some("main_btn"));
        assert_eq!(fm.focused(), Some("main_btn"));
    }

    #[test]
    fn set_focus_null_clears() {
        let mut fm = fm_with(&["main_btn"]);
        let _ = fm.focus_set("main_btn");
        let out = focus_set(&mut fm, &FocusSetParams { tag: None }).unwrap();
        assert!(out.focused.is_none());
        assert!(fm.focused().is_none());
    }

    #[test]
    fn set_focus_unknown_tag_errors() {
        let mut fm = fm_with(&["main_btn"]);
        let err = focus_set(
            &mut fm,
            &FocusSetParams { tag: Some("bogus".to_owned()) },
        )
        .unwrap_err();
        assert_eq!(err, FocusError::NotFocusable("bogus".to_owned()));
    }

    #[test]
    fn set_focus_already_focused_is_no_op_success() {
        let mut fm = fm_with(&["main_btn"]);
        let _ = focus_set(
            &mut fm,
            &FocusSetParams { tag: Some("main_btn".to_owned()) },
        );
        let out = focus_set(
            &mut fm,
            &FocusSetParams { tag: Some("main_btn".to_owned()) },
        )
        .unwrap();
        // Repeated set returns the same state, not an error.
        assert_eq!(out.focused.as_deref(), Some("main_btn"));
    }

    #[test]
    fn get_focus_returns_state_and_tab_order() {
        let mut fm = fm_with(&["a", "b", "c"]);
        let _ = fm.focus_set("b");
        let state = focus_get(&fm);
        assert_eq!(state.focused.as_deref(), Some("b"));
        let order = state.tab_order.expect("populated by focus_get");
        assert_eq!(order, vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
    }

    #[test]
    fn get_focus_with_empty_manager() {
        let fm = FocusManager::new();
        let state = focus_get(&fm);
        assert!(state.focused.is_none());
        let order = state.tab_order.expect("populated by focus_get");
        assert!(order.is_empty());
    }
}
