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

use crate::dispatch::RpcError;
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
            // from the active order OR already focused. Distinguish the
            // two by checking membership explicitly so the no-op
            // (already-focused) case returns success.
            //
            // R693 §5.39 — check the *active* order, not the base
            // `tab_order`: while a modal trap is up the addressable
            // targets are the scope's members (the dialog's controls,
            // absent from the static `tab_order`), and the base
            // focusables (the invoker behind the scrim) are not
            // addressable. This confines an AI client's `focus/set` to
            // the modal exactly as it confines Tab.
            let in_order = focus.active_tab_order().iter().any(|t| t == tag);
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
        // R693 §5.39 — report the *active* order so an AI client sees
        // the modal trap's enumeration (the dialog controls) while a
        // dialog is open, and the base `tab_order` otherwise.
        tab_order: Some(focus.active_tab_order().to_vec()),
    }
}

/// R51.74 §5.40 — `focus/next`. Tab-equivalent for AI clients.
///
/// Moves focus to the next element in
/// [`pinion_runtime::FocusManager::tab_order`], wrapping end →
/// start. When focus is unset, focuses the first element (matches
/// the ARIA Authoring Practices first-Tab convention). Returns the
/// new focus state.
pub fn focus_next(focus: &mut FocusManager) -> FocusState {
    let _ = focus.focus_next();
    FocusState {
        focused: focus.focused().map(str::to_owned),
        tab_order: None,
    }
}

/// R51.74 §5.40 — `focus/prev`. Shift+Tab-equivalent for AI clients.
///
/// Moves focus to the previous element, wrapping start → end. When
/// focus is unset, focuses the last element (ARIA Authoring
/// Practices first-Shift+Tab convention).
pub fn focus_prev(focus: &mut FocusManager) -> FocusState {
    let _ = focus.focus_prev();
    FocusState {
        focused: focus.focused().map(str::to_owned),
        tab_order: None,
    }
}

/// R51.85 §5.40 — JSON-RPC error for "no focus manager wired".
///
/// All four `focus/*` adapters fail on `Option<&FocusManager> = None`
/// with the same shape: the embedding shell did not register a focus
/// manager in [`crate::DispatchContext`]. Sharing a single constructor
/// keeps the error code + message in lockstep (`-32004` is reserved
/// for "service unavailable" in pinion-rpc's error space).
///
/// R51.89 §5.40 — routed through [`RpcError::new`]; the
/// `code/message/data: None` triple is no longer hand-built here.
fn err_focus_unavailable() -> RpcError {
    RpcError::new(-32004, "focus manager unavailable")
}

/// R51.85 §5.40 — typed→JSON lift for [`FocusState`] responses.
/// Centralises the `to_value → -32603` mapping the four handlers
/// share.
///
/// R51.89 §5.40 — uses [`RpcError::internal_error`] in place of the
/// retired `err_internal` local helper.
fn state_to_value(state: FocusState) -> Result<Value, RpcError> {
    serde_json::to_value(state).map_err(RpcError::internal_error)
}

/// R51.85 §5.40 — lift a [`FocusError`] into the dispatcher's
/// `RpcError` shape. `Unavailable` mirrors
/// [`err_focus_unavailable`]; `NotFocusable` carries the offending
/// tag in `data` so AI clients can branch on the rejected name.
///
/// R51.89 §5.40 — `NotFocusable` builds via
/// [`RpcError::new`] + [`RpcError::with_data_string`]; the literal
/// `RpcError { code, message, data }` shape stays out of focus.rs.
fn err_from_focus(e: FocusError) -> RpcError {
    match e {
        FocusError::Unavailable => err_focus_unavailable(),
        FocusError::NotFocusable(tag) => {
            RpcError::new(-32602, "tag_not_focusable").with_data_string(tag)
        }
    }
}

/// JSON-RPC adapter for `focus/set`. Parses the `params` Value into
/// [`FocusSetParams`], runs [`focus_set`], and lifts the result into
/// the dispatcher's `(serde_json::Value, RpcError)` shape.
///
/// R51.89 §5.40 — `params` parse failures and serializer failures
/// route through [`RpcError::invalid_params`] / [`RpcError::internal_error`]
/// (former local helpers `err_invalid_params` / `err_internal` retired).
pub(crate) fn handle_focus_set(
    focus: Option<&mut FocusManager>,
    params: Option<&Value>,
) -> Result<Value, RpcError> {
    let focus = focus.ok_or_else(err_focus_unavailable)?;
    let parsed: FocusSetParams = match params {
        Some(p) => serde_json::from_value(p.clone()).map_err(RpcError::invalid_params)?,
        None => FocusSetParams { tag: None },
    };
    let state = focus_set(focus, &parsed).map_err(err_from_focus)?;
    state_to_value(state)
}

/// JSON-RPC adapter for `focus/get`.
pub(crate) fn handle_focus_get(focus: Option<&FocusManager>) -> Result<Value, RpcError> {
    let focus = focus.ok_or_else(err_focus_unavailable)?;
    state_to_value(focus_get(focus))
}

/// JSON-RPC adapter for `focus/next` (R51.74 §5.40).
pub(crate) fn handle_focus_next(focus: Option<&mut FocusManager>) -> Result<Value, RpcError> {
    let focus = focus.ok_or_else(err_focus_unavailable)?;
    state_to_value(focus_next(focus))
}

/// JSON-RPC adapter for `focus/prev` (R51.74 §5.40).
pub(crate) fn handle_focus_prev(focus: Option<&mut FocusManager>) -> Result<Value, RpcError> {
    let focus = focus.ok_or_else(err_focus_unavailable)?;
    state_to_value(focus_prev(focus))
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
            &FocusSetParams {
                tag: Some("main_btn".to_owned()),
            },
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
            &FocusSetParams {
                tag: Some("bogus".to_owned()),
            },
        )
        .unwrap_err();
        assert_eq!(err, FocusError::NotFocusable("bogus".to_owned()));
    }

    #[test]
    fn set_focus_already_focused_is_no_op_success() {
        let mut fm = fm_with(&["main_btn"]);
        let _ = focus_set(
            &mut fm,
            &FocusSetParams {
                tag: Some("main_btn".to_owned()),
            },
        );
        let out = focus_set(
            &mut fm,
            &FocusSetParams {
                tag: Some("main_btn".to_owned()),
            },
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

    #[test]
    fn focus_next_advances_in_tab_order() {
        let mut fm = fm_with(&["a", "b", "c"]);
        let _ = fm.focus_set("a");
        let state = focus_next(&mut fm);
        assert_eq!(state.focused.as_deref(), Some("b"));
    }

    #[test]
    fn focus_next_wraps_at_end() {
        let mut fm = fm_with(&["a", "b"]);
        let _ = fm.focus_set("b");
        let state = focus_next(&mut fm);
        assert_eq!(state.focused.as_deref(), Some("a"));
    }

    #[test]
    fn focus_next_from_unfocused_picks_first() {
        let mut fm = fm_with(&["a", "b", "c"]);
        let state = focus_next(&mut fm);
        assert_eq!(state.focused.as_deref(), Some("a"));
    }

    #[test]
    fn focus_prev_steps_backward() {
        let mut fm = fm_with(&["a", "b", "c"]);
        let _ = fm.focus_set("c");
        let state = focus_prev(&mut fm);
        assert_eq!(state.focused.as_deref(), Some("b"));
    }

    #[test]
    fn focus_prev_wraps_at_start() {
        let mut fm = fm_with(&["a", "b"]);
        let _ = fm.focus_set("a");
        let state = focus_prev(&mut fm);
        assert_eq!(state.focused.as_deref(), Some("b"));
    }

    #[test]
    fn focus_prev_from_unfocused_picks_last() {
        let mut fm = fm_with(&["a", "b", "c"]);
        let state = focus_prev(&mut fm);
        assert_eq!(state.focused.as_deref(), Some("c"));
    }

    #[test]
    fn focus_next_empty_tab_order_returns_none() {
        let mut fm = FocusManager::new();
        let state = focus_next(&mut fm);
        assert!(state.focused.is_none());
    }
}
