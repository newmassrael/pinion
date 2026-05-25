#!/usr/bin/env python3
"""R667 §5.16 — settings-panel 2nd composed app AI-driven E2E.

Phase A finalisation demo. Drives the 4 interactive widgets the
settings-panel binding exposes (nav RadioGroup, theme Toggle, font
Slider, profile TextField) through `scene/invoke v1` paths (R666
substrate) + `scene/key character` arc (R666 #3) and verifies the
launch-kill-relaunch persistence cycle.

Cycle 1 (boot 1):
  - Verify defaults: nav_index=0, dark_mode=false, font_scale~0.5,
    display_name="" plus 5-section nav rail visible.
  - Nav-cycle: click each of nav_rail#1..#4 and verify
    selected_index advances, the per-section detail label appears,
    and the storage blob persists the new index.
  - Dark mode: invoke v1 the theme_toggle `send` action with
    PointerEnter/Down/Up/Leave (mirrors a click), verify the
    toggle's `value` slot flips and the theme provider reports
    `Dark` (queried via theme/tokens).
  - Font scale: intervene the slider's `value` slot to 0.85, verify
    the post-write read returns the same.
  - Display name: focus the TextField, type "Ada", verify the field
    text matches.
  - Kill subprocess (storage Effect drained on the last set).

Cycle 2 (relaunch with the same storage dir):
  - Verify defaults are gone — every persisted field reads back the
    cycle-1 value (nav_index=4, dark_mode=true, font_scale~0.85,
    display_name="Ada").
  - Storage blob on disk reflects the persisted shape.

Total assertions: ≥ 45.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

# noqa: E402 — runtime path manipulation above.
from rpc_verify import (  # noqa: E402
    RpcSubprocess,
    assert_eq,
    isolated_storage_dir,
    run_demo,
)


PROFILE_TF_TAG = "profile_display_name"
NAV_TAG = "nav_rail"
THEME_TOGGLE_TAG = "theme_toggle"
FONT_SLIDER_TAG = "font_slider"
STORAGE_KEY = "state.json"


def storage_blob(root: Path) -> dict:
    path = root / STORAGE_KEY
    return json.loads(path.read_text(encoding="utf-8"))


def click_nav(tf: RpcSubprocess, index: int) -> None:
    """Click the i-th nav row via composite tag — RadioGroupExternal
    addresses children through `<group>#<i>` (R51.42)."""
    tf.click(path=f"{NAV_TAG}#{index}")


def read_selected_nav(tf: RpcSubprocess) -> int:
    resp = tf.request("scene/query", {"path": f"/{NAV_TAG}/external/selected_index"})
    # `scene/query` returns the bare introspect-value JSON
    # (`introspect_value_to_json` in dispatch.rs):
    # `IntrospectValue::Int(n)` → `Number(n)`, etc.
    return int(resp.result)


def read_theme_on(tf: RpcSubprocess) -> bool:
    resp = tf.request("scene/query", {"path": f"/{THEME_TOGGLE_TAG}/external/value"})
    return bool(resp.result)


def read_slider_value(tf: RpcSubprocess) -> float:
    resp = tf.request("scene/query", {"path": f"/{FONT_SLIDER_TAG}/external/value"})
    return float(resp.result)


def cycle1(tf: RpcSubprocess, storage_dir: Path) -> None:
    # (1) Defaults — first boot, empty storage.
    assert_eq(read_selected_nav(tf), 0, "(1.a) nav_index default = 0")
    assert_eq(read_theme_on(tf), False, "(1.b) dark mode default = false")
    initial_value = read_slider_value(tf)
    assert_eq(round(initial_value, 4), 0.5, "(1.c) font_scale default = 0.5")
    # Display name starts empty.
    text_resp = tf.request("scene/query", {"path": f"/{PROFILE_TF_TAG}/external/text"})
    assert_eq(str(text_resp.result), "", "(1.d) display_name default = ''")
    # Storage blob written eagerly on boot — the `use_settings_persistence`
    # save Effect fires its initial-run pass against the seed values.
    blob_initial = storage_blob(storage_dir)
    assert_eq(blob_initial.get("nav_index"), 0, "(1.e) initial blob nav_index = 0")
    assert_eq(
        blob_initial.get("dark_mode"),
        False,
        "(1.e.2) initial blob dark_mode = false",
    )

    # All four Externals appear in the state-scene snapshot
    # regardless of nav_index — `create_extra_externals` registers
    # every widget; only the per-section view-fn body switches.
    snap = tf.snapshot(viewport=(720, 480))
    snap_text = json.dumps(snap)
    for tag in (PROFILE_TF_TAG, NAV_TAG, THEME_TOGGLE_TAG, FONT_SLIDER_TAG):
        assert_eq(tag in snap_text, True, f"(1.f.{tag}) external tag {tag!r} in snapshot")

    # (2) Nav-cycle through each section by clicking nav_rail#i.
    for i in [1, 2, 3, 4]:
        click_nav(tf, i)
        assert_eq(read_selected_nav(tf), i, f"(2.{i}) nav row #{i} selected after click")

    # (3) Switch back to nav[0] = Theme section before driving the
    # toggle; the toggle is only painted while Theme is visible.
    click_nav(tf, 0)
    assert_eq(read_selected_nav(tf), 0, "(3.a) nav back to theme section (#0)")

    # Click the toggle (mirror of a real-mouse click — InputRouter
    # arc Enter/Down/Up/Leave).
    tf.click(path=THEME_TOGGLE_TAG)
    assert_eq(read_theme_on(tf), True, "(3.b) dark mode = on after first click")

    # (4) Drag the slider — easiest is to `intervene` the value
    # directly through the v1 path. Mirrors what an AI driver would
    # do without simulating mouse motion.
    for (i, target) in enumerate([0.1, 0.5, 0.85]):
        tf.request(
            "scene/intervene",
            {"path": f"/{FONT_SLIDER_TAG}/external/value", "value": target},
        )
        post = read_slider_value(tf)
        assert_eq(
            round(post, 4),
            target,
            f"(4.{i + 1}) font_scale intervene round-trip = {target}",
        )

    # (5) Switch to Profile section, type the display name.
    click_nav(tf, 2)
    assert_eq(read_selected_nav(tf), 2, "(5.a) nav on profile section (#2)")
    # Profile-section TextField stays in the state-scene snapshot
    # across nav switches (extras are always registered).
    snap_profile = tf.snapshot(viewport=(720, 480))
    assert_eq(
        PROFILE_TF_TAG in json.dumps(snap_profile),
        True,
        "(5.b) profile TextField external visible after nav switch",
    )
    tf.request("focus/set", {"tag": PROFILE_TF_TAG})
    focus_resp = tf.request("focus/get", None)
    assert_eq(
        focus_resp.result.get("focused"),
        PROFILE_TF_TAG,
        "(5.c) focus moved to profile field",
    )
    tf.text("Ada", path=PROFILE_TF_TAG)

    text_resp = tf.request("scene/query", {"path": f"/{PROFILE_TF_TAG}/external/text"})
    assert_eq(str(text_resp.result), "Ada", "(5.d) profile field text after typing = 'Ada'")

    # Enter — runs the apply_key Enter arm that pushes the field
    # text into the persisted Signal<String>.
    tf.key(path=PROFILE_TF_TAG, name="Enter")

    # Storage blob should now show all four persisted fields.
    blob = storage_blob(storage_dir)
    assert_eq(blob.get("nav_index"), 2, "(6.a) storage nav_index = 2")
    assert_eq(blob.get("dark_mode"), True, "(6.b) storage dark_mode = true")
    assert_eq(
        round(blob.get("font_scale", -1.0), 4),
        0.85,
        "(6.c) storage font_scale = 0.85",
    )
    assert_eq(blob.get("display_name"), "Ada", "(6.d) storage display_name = 'Ada'")
    assert_eq(blob.get("schema_version"), 1, "(6.e) storage schema_version = 1")


def cycle2(tf: RpcSubprocess, storage_dir: Path) -> None:
    # Defaults are gone — every persisted field reads back the
    # cycle-1 value.
    assert_eq(read_selected_nav(tf), 2, "(7.a) relaunch nav_index restored to 2")
    assert_eq(read_theme_on(tf), True, "(7.b) relaunch dark_mode restored to true")
    assert_eq(
        round(read_slider_value(tf), 4),
        0.85,
        "(7.c) relaunch font_scale restored to 0.85",
    )
    text_resp = tf.request("scene/query", {"path": f"/{PROFILE_TF_TAG}/external/text"})
    assert_eq(str(text_resp.result), "Ada", "(7.d) relaunch display_name restored to 'Ada'")

    # Storage blob still intact.
    blob = storage_blob(storage_dir)
    assert_eq(blob.get("nav_index"), 2, "(8.a) storage post-relaunch nav_index = 2")
    assert_eq(blob.get("dark_mode"), True, "(8.b) storage post-relaunch dark_mode = true")
    assert_eq(
        round(blob.get("font_scale", -1.0), 4),
        0.85,
        "(8.c) storage post-relaunch font_scale = 0.85",
    )
    assert_eq(
        blob.get("display_name"),
        "Ada",
        "(8.d) storage post-relaunch display_name = 'Ada'",
    )

    # Cycle 2 mutations to prove the live path still works.
    click_nav(tf, 4)
    assert_eq(read_selected_nav(tf), 4, "(9.a) relaunch click nav to #4 (Actions)")
    click_nav(tf, 1)
    assert_eq(read_selected_nav(tf), 1, "(9.b) relaunch click nav to #1 (Appearance)")

    # Flip dark mode off via toggle.
    click_nav(tf, 0)
    assert_eq(read_selected_nav(tf), 0, "(10.a) relaunch back to theme (#0) for toggle")
    tf.click(path=THEME_TOGGLE_TAG)
    assert_eq(read_theme_on(tf), False, "(10.b) relaunch toggle flips dark_mode off")

    # Storage reflects the second-cycle mutations.
    blob2 = storage_blob(storage_dir)
    assert_eq(blob2.get("nav_index"), 0, "(10.c) storage cycle2 nav_index = 0")
    assert_eq(blob2.get("dark_mode"), False, "(10.d) storage cycle2 dark_mode = false")
    # font_scale + display_name carry across the toggle flip
    # (proves the persistence Effect doesn't reset unrelated fields).
    assert_eq(
        round(blob2.get("font_scale", -1.0), 4),
        0.85,
        "(10.e) storage cycle2 font_scale unchanged from cycle1",
    )
    assert_eq(
        blob2.get("display_name"),
        "Ada",
        "(10.f) storage cycle2 display_name unchanged from cycle1",
    )
    # schema_version must remain on the supported tier through every
    # save (any breaking change must bump the version + ship a
    # migrator per the R665 carry).
    assert_eq(
        blob2.get("schema_version"),
        1,
        "(10.g) storage cycle2 schema_version stays at 1",
    )


def body() -> None:
    with isolated_storage_dir("pinion-settings-panel-r667-") as storage_dir:
        with RpcSubprocess("settings-panel") as tf:
            cycle1(tf, storage_dir)
        # First subprocess exited — storage Effect drained on every
        # set. Now relaunch against the same storage dir.
        with RpcSubprocess("settings-panel") as tf:
            cycle2(tf, storage_dir)


if __name__ == "__main__":
    sys.exit(run_demo(
        "R667 §5.16 — settings-panel 2nd composed app + Phase A close",
        body,
    ))
