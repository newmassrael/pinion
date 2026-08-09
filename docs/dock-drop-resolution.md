# Dock drop resolution — the discrete-target / float-first-class model (B)

Status: design + R1159 foundation. Supersedes the implicit `over.is_none()`
float decision scattered across the dock drag handlers.

## Why

The pre-B model classified **every** point inside a panel into a dock zone
(`dock_drop_zone_normalized`: a centre square → tabify, the surrounding frame →
nearest-edge split). Float was decided separately, by `over.is_none()` — i.e. the
cursor escaped **every** panel, which in a tiled layout means **outside the
window**. Two structural failures fell out of this:

1. **Float is impossible in a maximized / fullscreen single-monitor window.** The
   cursor cannot leave the window, so `over` is never `None`, so a tab/panel can
   never float by dragging. (The live bug the user hit: "탭이 창 밖으로 안 빠져".)
2. **A release over a non-panel (a splitter, a container) no-ops.** The release
   handler called `dock_panel_at_zone(panel, over.tag, …)` without checking the
   target is a dockable panel, so a drop over `editor_split_middle_h` silently did
   nothing.

This is not a small bug. The continuous-zone model is the **region-highlight**
family (VS Code's editor groups). The north star is an **engine-class editor**;
the engine Slate / Visual Studio / the toolkit Advanced Docking all use the **discrete
drop-target + float-on-release-off-target** model, where float is a first-class
in-window outcome.

## The model (B): discrete targets, float is first-class

A drag release resolves to exactly one of:

```
DropResolution =
  | Dock      { target: PanelId, zone: DockDropZone }  // edge → split, centre → tabify
  | OuterDock { edge: DockDropZone }                   // full-span perimeter dock (R1156)
  | Float                                              // release off every target
```

**One resolver is the SSOT** for "what does releasing here do":

```
resolve_drop(over: Option<&DropPoint>, is_panel: Fn(&str)->bool, tabbing: bool)
    -> DropResolution
```

* `over == None` (cursor over no drop target) → `Float`.
* `over.tag == OUTER_DOCK_ZONE_TAG` → `OuterDock { edge }` (R1156 perimeter).
* `over.tag` (split at `#`) is **not** a dockable panel (a splitter / container)
  → `Float`. This kills the splitter no-op.
* else classify the panel-relative cursor with the **banded** geometry:
  * within `DOCK_SPLIT_BAND_FRAC` of an edge → that edge (`Dock`, split),
  * within `DOCK_CENTER_HALF_FRAC` of centre (Chebyshev) and `tabbing` → `Center`
    (`Dock`, tabify),
  * **the dead-zone ring between** → `Float`.

The dead-zone ring is what makes float reachable **inside** the window (and so in
a maximized window): aim at an edge to split, at the centre to tab, anywhere in
the neutral ring (or off a panel) to float.

### Geometry constants (R1159 start; tune live)

* `DOCK_SPLIT_BAND_FRAC = 0.22` — edge split band.
* `DOCK_CENTER_HALF_FRAC = 0.18` — centre tabify square half-extent.
* The ring between (~0.22..0.32 from each edge) is the float dead-zone.

These are a **new** geometry (`dock_drop_zone_banded`), distinct from the legacy
continuous `dock_drop_zone_normalized` (kept until every consumer migrates), so
no caller silently changes behaviour mid-migration.

## Preview == result, by construction

The drag preview and the applied edit must never disagree (the R1149 lesson). So
the **preview is derived from `resolve_drop`** too: `Dock` → the zone overlay on
the target panel, `OuterDock` → the full-span overlay, `Float` → no dock overlay
(a float affordance instead). Every drag source's preview and result flow from
the one resolver.

## Consumers (all funnel through `resolve_drop`)

* **Tab drag** (`TabWellExternal`) — R1159 (the active consumer + the live bug).
* **Panel-header drag** (`DockPanelExternal`) — R1160 (unify; clear the
  tab-vs-panel divergence the same session).
* **Cross-window / outer** (`resolve_cross_window_drop` in the shell) — already
  emits the OUTER sentinel + a per-window `DropPoint`; audited into `resolve_drop`
  in R1162.
* **RPC reorganize** (`DockReorganizeExternal`) — names a zone explicitly; stays
  an explicit-zone path (not a cursor resolution).

## Discoverability — guides always on (R1161)

The discrete targets are only useful if visible. `dock_zone_guide` overlays today
gate on `any_other_window_dragging` (cross-window only). R1161 turns them on for
**same-window** drags too, so a tab/panel drag shows the edge/centre band targets
(and the dead-zone implicitly = float).

## Round plan

| Round | Scope |
|---|---|
| R1159 | design doc + `DropResolution` + `resolve_drop` + banded geometry + unit tests + route the **tab** (preview + result) + demo + live XTEST verify |
| R1160 | route the **panel-header** drag through `resolve_drop` (unify; clear divergence) |
| R1161 | guides always-on for same-window drags (discoverability) |
| R1162 | audit cross-window + outer through `resolve_drop`; retire legacy `dock_drop_zone_normalized` once every consumer migrated |

Each round: workspace tests green + clippy `-D pedantic` + rustfmt + the touched
demo (+ XTEST live verify for the gesture rounds). The legacy continuous
classifier is retired only when its last consumer migrates (no orphan).
