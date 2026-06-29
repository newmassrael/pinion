# Dock window-move redesign — the VS Code model (release-only)

> Design of record for the R1146 dock-completion round. Surfaced by the
> R1135–R1145 floater-redock-UX session review: the floater drag was
> **app-driven** (it created + moved a REAL OS window every cursor frame), which
> hangs live and forced a cascade of symptom-patches (R1143/R1144/R1145). This
> doc records the textbook fix and what it deletes.

## The defect

A dragged dock panel relocates a real OS window on **every cursor move**:

- **Tear-off** (a docked panel dragged out): `drag_to_at` → `enqueue_tear_off_follow`
  on every escaped move → editor `follow_panel_floating` → `Signal::set` →
  shell `reconcile_windows`. The FIRST move *creates* a winit window (an
  expensive `pollster::block_on(VelloRenderer::new(..))` GPU-surface init), and
  every later move calls `Window::set_outer_position`. Painting the brand-new,
  surface-churning floater under an active drag triggered a wgpu validation
  cascade that **froze** the window (R1144's stated root cause).
- **Floater move** (a borderless floater's title-bar drag): `drag_to_at` →
  `enqueue_window_move(delta)` on every move → editor `move_floating_window` →
  `Signal::set` → `set_outer_position`. A fast manual drag floods the WM
  (`mutter` on `:0`) with `set_outer_position` and wedges the event loop — the
  live **hang**.

The defect is invisible to the test suite by construction: the RPC drive uses
`PINION_HIDDEN_WINDOW`, so there is no real WM window to create/move, so even a
300-step `scene/drag` returns in ~0s.

The whole R1118–R1120 "desktop-frame delta to break the feedback oscillation"
machinery exists ONLY because the window moves per-frame under WM apply-lag.
Remove the per-frame move and that feedback loop cannot exist.

## The fix: preview during the drag, real window only on release

Pro dockers (VS Code / Qt ADS / Blender / Unreal) show a **lightweight preview**
that follows the cursor and touch the real window **once, on release**. pinion
already has every preview piece, all driven purely from the router's live drag
session (cursor, source window, cross-window drop) — confirmed independent of
any real floating window:

- **R1113 drag-image follower** (shell overlay, `apply_drag_image`) — the ghost
  chip at the window-local cursor.
- **R1141 dock-zone guides** (`apply_dock_zone_guides`) — the host window's drop
  zones highlighted so the user sees where to aim.
- **R1137 redock preview** (`apply_redock_drag_hint`) — the resolved zone painted
  into the dragged floater.

So the binding stops emitting window-mutating intents per move; it emits exactly
one on release:

- **During the drag** (`drag_to_at`): drive the redock-armed lifecycle + the
  drop preview ONLY. No `tear_off_follow`, no `window_move`. The leaf stays live
  (the panel has not floated yet), the ghost + guides + redock preview track the
  cursor.
- **On release** (`drag_release` / `drag_release_at`):
  - Over a dock zone → re-dock via the existing topology ops (reorganize / split
    / cross-window redock). No floating window is ever created.
  - Off every zone, tear-off → `enqueue_tear_off_follow` ONCE at the release
    cursor: the editor creates the floating window once at the drop point
    (`WindowSpec.position`, applied at create — never per-move).
  - Off every zone, floater move → `enqueue_window_move(release − press)` ONCE:
    the floater repositions a single time. Because the window did not move during
    the drag, `window_move_delta(None, cursor, press, None)` reduces to
    `cursor − press` (the `actual_origin` term cancels) — no per-move bookkeeping.

The editor reducer (`follow_panel_floating`, `move_floating_window`) is unchanged
— it is already idempotent; it is simply now called once instead of per move.

### Consequence accepted deliberately

A floater's free-move (dragging it to a new desktop spot, not redocking) now
**settles on release** rather than tracking smoothly mid-drag. That is the VS
Code panel-drag model and it is the price of zero `set_outer_position` flood. The
smooth alternative — the WM's native interactive move (`Window::drag_window` /
`_NET_WM_MOVERESIZE`) — is rejected here because it surrenders cursor observation
during the move, which would break redock detection (the entire point of the
R1136–R1145 campaign). Redock-on-drag wins; smooth free-move is a possible later
affordance on a separate grab target.

## What this deletes (band-aids the redesign makes unnecessary)

- **R1143 center-fallback** (`resolve_cross_window_live` body-centre second
  resolution point) — a dual resolution-point heuristic compensating for the
  *moving* floater occluding the cursor target. With a stationary floater the
  cursor resolution is precise (`abs = floater_pos + cursor_local` tracks the
  true desktop pointer), so the single cursor point is correct and textbook;
  resolving at the stationary floater's centre would now point at the wrong
  place. Deleted.
- **R1144 size-gate** (`smaller_window_dragging` / `window_area`, and the
  larger-than-source release-repaint gate) — existed solely to avoid painting
  guides on the churning mid-drag floater. No mid-drag floater exists anymore, so
  the gate is dead; worse, it would suppress the guides during a same-window
  tear-off (where MAIN is the dragger). Replaced by a plain "a drag is in flight"
  gate so guides show during both tear-off and floater-redock.

R1145 (undock-tab button) is a redundant affordance once the natural tab drag
works; its deletion is deferred until drag-to-undock is live-verified (the
result differs — drag floats / re-splits; the button undocks-to-split).

## Verification

- Controlled unit tests: a multi-step drag emits **zero** `tear_off_follow` /
  `window_move` intents mid-drag and **exactly one** window-mutating intent on
  release (this is the headless-observable proof that the flood is gone).
- The existing redock / escape-float / snap-back outcomes are unchanged (the
  release classification is untouched).
- The live *feel* (no hang, no freeze, smooth ghost, redock feel) is HW-gated —
  the user verifies on their real desktop (`:0` / mutter). The flood/freeze it
  fixes was itself only observable live, so the unit proof is "no per-move
  window intent"; the absence of the live hang is the user's confirmation.
