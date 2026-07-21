#!/usr/bin/env python3
"""R1404 §5.16 — a producer-supplied in-memory image source (`memory://<key>`).

Before R1404 a `Scene::Image` source was a filesystem path only, so a producer
that decoded an image at runtime — a terminal's Kitty-graphics / sixel raster,
an app-generated bitmap — could not hand pinion those pixels. R1404 adds the
`MemoryImageStore`: the shell seeds one at root, hands the handle to every
window's `ImageCache`, and a producer registers decoded RGBA under a key. A
`Scene::Image { source: "memory://<key>" }` node then paints it, GPU-backed and
headless alike, with no file. The store is MUTABLE (re-register / remove), the
Kitty-animation / retransmit / delete a terminal image needs.

The proof is pure DATA over RPC. `scene/snapshot` reports the image node with
`source: "memory://tile"` (the AI sees the memory scheme in the scene), and the
node STAYS put across every mutation — only the pixels behind it change, which a
snapshot cannot see. The primary `MemoryImageOracle` reports the store state a
snapshot cannot: `variant` (which palette), `present` (is the key registered),
`registered` (0 or 1), `width` / `height`. A client drives the mutation with no
pixel — `invoke send "swap"/"remove"/"restore"`, or `intervene variant` /
`intervene present` — and reads the effect back. The GPU pixel witness (the
image really rasterizes, and a re-register/remove is visible next frame) is the
`#[ignore]`d `r1404_memory_scheme_image_paints_and_mutates` lavapipe test.

Run from the workspace root:
    cargo build -p hello-memory-image --release
    python3 tools/demos/r1404_memory_image.py
"""

from __future__ import annotations

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent.parent))

from rpc_verify import (
    RpcSubprocess,
    assert_eq,
    find_by_tag,
    run_demo,
    wait_snap,
)

WIN = (360, 320)
ORACLE = "/external"
IMG_TAG = "tile_image"
SOURCE = "memory://tile"


def img(snap) -> dict:
    node = find_by_tag(snap, IMG_TAG)
    assert node is not None, "the memory:// image node is in the paint scene"
    return node


def wait_image(tf, desc: str):
    """Wait until the memory-sourced image node has resolved in the paint scene."""
    return wait_snap(
        tf,
        lambda s: (find_by_tag(s, IMG_TAG) or {}).get("source") == SOURCE,
        source="paint",
        viewport=WIN,
        desc=desc,
    )


def body() -> None:
    with RpcSubprocess("hello-memory-image") as tf:
        snap = wait_image(tf, "boot: memory:// image node resolved")

        # --- boot: the scene declares a memory:// source; the oracle reports
        #     the producer registered the 'cool' palette under it. ---
        node = img(snap)
        assert_eq(node["source"], SOURCE, "image node source is memory://tile")
        assert_eq(node["tag"], IMG_TAG, "image node tag")
        assert_eq(node["style"]["fit"], "Fill", "image fits the cell (Fill)")
        assert_eq(tf.query(f"{ORACLE}/image_source"), SOURCE, "oracle image_source")
        assert_eq(tf.query(f"{ORACLE}/image_key"), "tile", "oracle image_key")
        assert_eq(tf.query(f"{ORACLE}/variant"), "cool", "boot palette is cool")
        assert_eq(tf.query(f"{ORACLE}/present"), True, "boot: the key is registered")
        assert_eq(tf.query(f"{ORACLE}/registered"), 1, "boot: one image registered")
        assert_eq(tf.query(f"{ORACLE}/width"), 16, "boot image width 16")
        assert_eq(tf.query(f"{ORACLE}/height"), 16, "boot image height 16")

        # --- swap: re-register the OTHER palette under the same key (a mutable
        #     update). The scene's source is UNCHANGED — only the pixels move. ---
        assert_eq(tf.invoke(f"{ORACLE}/send", "swap"), "warm", "swap returns the new palette")
        assert_eq(tf.query(f"{ORACLE}/variant"), "warm", "swapped to warm")
        assert_eq(tf.query(f"{ORACLE}/present"), True, "still registered after a swap")
        assert_eq(tf.query(f"{ORACLE}/registered"), 1, "still one image after a swap")
        after_swap = wait_image(tf, "after swap: the image node persists")
        assert_eq(img(after_swap)["source"], SOURCE, "source stable across a swap (pixels moved, not the node)")

        # --- remove: delete the key. The node STAYS in the scene but now
        #     resolves to nothing (paints the cell background). ---
        assert_eq(tf.invoke(f"{ORACLE}/send", "remove"), "absent", "remove reports absent")
        assert_eq(tf.query(f"{ORACLE}/present"), False, "removed: not registered")
        assert_eq(tf.query(f"{ORACLE}/registered"), 0, "removed: store is empty")
        assert_eq(tf.query(f"{ORACLE}/width"), 0, "removed: no image dims")
        assert_eq(tf.query(f"{ORACLE}/height"), 0, "removed: no image dims")
        gone = wait_image(tf, "after remove: the image node still present")
        assert_eq(img(gone)["source"], SOURCE, "the node persists when the source is unregistered")

        # --- restore: re-register the last (warm) palette. ---
        assert_eq(tf.invoke(f"{ORACLE}/send", "restore"), "warm", "restore brings back warm")
        assert_eq(tf.query(f"{ORACLE}/present"), True, "restored: registered again")
        assert_eq(tf.query(f"{ORACLE}/registered"), 1, "restored: one image")
        assert_eq(tf.query(f"{ORACLE}/variant"), "warm", "restored the last palette")

        # --- AI-first no-pixel channel: intervene variant / present ---
        tf.intervene(f"{ORACLE}/variant", "cool")
        assert_eq(tf.query(f"{ORACLE}/variant"), "cool", "intervene variant sets the palette")
        assert_eq(tf.query(f"{ORACLE}/present"), True, "intervene variant registers it")

        tf.intervene(f"{ORACLE}/present", False)
        assert_eq(tf.query(f"{ORACLE}/present"), False, "intervene present=false removes it")
        assert_eq(tf.query(f"{ORACLE}/registered"), 0, "intervene present=false empties the store")

        tf.intervene(f"{ORACLE}/present", True)
        assert_eq(tf.query(f"{ORACLE}/present"), True, "intervene present=true restores it")
        assert_eq(tf.query(f"{ORACLE}/registered"), 1, "intervene present=true re-registers")
        assert_eq(tf.query(f"{ORACLE}/variant"), "cool", "restored the current (cool) palette")

        # --- explicit palette sends ---
        assert_eq(tf.invoke(f"{ORACLE}/send", "warm"), "warm", "send warm")
        assert_eq(tf.query(f"{ORACLE}/variant"), "warm", "warm is registered")
        assert_eq(tf.invoke(f"{ORACLE}/send", "cool"), "cool", "send cool")
        assert_eq(tf.query(f"{ORACLE}/variant"), "cool", "cool is registered")

        # --- the scene source is invariant through all of it (the AI's stable
        #     handle; the pixels behind it are what changed). ---
        final = wait_image(tf, "final: the memory:// image node is unchanged")
        assert_eq(img(final)["source"], SOURCE, "source is invariant across every mutation")


if __name__ == "__main__":
    sys.exit(run_demo("r1404_memory_image", body))
