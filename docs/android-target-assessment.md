# Android target — measured assessment and the `unsafe_code` boundary decision

> Status: **assessment + decision, not yet a round's output** (design session
> 2026-08-20, tree at R1741). Nothing here has been built. This doc is the
> Android-specific instantiation of `docs/cross-platform-native-strategy.md`
> (R932.2), which holds the general contract — one core, narrow trait seams,
> native last-mile, every difference queryable. Read that first; this does not
> restate it.
>
> **Supersedes the `mobile-target-deferred` audit snapshot (R679.3,
> 2026-05-27)** on every point below. That snapshot is ~1060 rounds old and
> two of its verdicts are now false — see "What the old audit got wrong".

## The decision

**pinion's `unsafe_code` boundary is drawn at the foreign thing, and the gate
is hung on the site — not on the crate.**

1. The only crate permitted to contain `unsafe` is **`pinion-jni`**. Its sole
   job is to make the JVM safe to call. It implements **no capability** —
   no clipboard, no storage, no file dialog. It declares its own
   `[lints.rust] unsafe_code = "deny"` and deliberately does **not** inherit
   `[lints] workspace = true`; each `unsafe` site carries a `#[allow]` and a
   SAFETY comment.
2. Every capability keeps its existing home. `pinion-platform-clipboard` /
   `-storage` / `-file-dialog` / `-fonts` grow their Android arm **inside
   themselves**, written against `pinion-jni`'s *safe* API, and they keep
   `unsafe_code = "forbid"`. **No per-platform crate is created** — the tree
   decomposes by capability, and a `pinion-platform-android` would give
   clipboard two homes, which is the debt shape this project keeps paying for.
3. The consuming edge is target-gated:
   `[target.'cfg(target_os = "android")'.dependencies]`. Measured: a crate
   reached only through such an edge is **not compiled for the host at all**,
   so the exception cannot reach the platforms shipping today.
4. A commit gate **counts `unsafe` occurrence sites and compares against a
   pin**. Crate-level permission is too coarse: the exception is a seam, not a
   subsystem, so the gate can afford to fix it by site. Opening the boundary
   later moves the pin from its current value with a recorded reason, which
   *is* the audit trail `Cargo.toml`'s lint comment asks for.
5. **Kotlin's share is decided later, per capability**, once the seam exists.
   Not decided now.
6. **Contingency:** if the Android entry symbol forces the exception into the
   final `cdylib` crate too (unknowable before a real Android link), the app
   crate becomes a thin shell holding nothing but the entry point, so the
   spread stops at one layer.

### Why this shape rather than the obvious one

The first draft of this decision was "one `pinion-platform-android` crate that
may use unsafe." Two things were wrong with it, both found by measuring:

- **The unsafe surface is a seam, not a subsystem.** `jni 0.21.1` is already a
  safe wrapper: the hot path (`attach_current_thread`, `get_env`,
  `call_method`) returns `Result` and is safe. What needs `unsafe` is the
  bootstrap — `JavaVM::from_raw` — essentially one expression, plus the entry
  symbol. So permitting a whole crate to do anything is wider than the need.
- **A per-platform crate cuts across the tree's decomposition axis.** Every
  `pinion-platform-*` crate is a *capability*. Adding a *platform* crate makes
  one fact live in two places, which this project has repeatedly registered as
  debt. Concentrating unsafe and preserving capability decomposition are not in
  tension once the concentrated crate wraps **the foreign thing** (the JVM)
  rather than **the platform's features**. This is the same shape the tree
  already uses: `pinion-a11y` is one boundary over accesskit, `pinion-gpu` one
  boundary over the wgpu device.

### The invariant this does not weaken

`unsafe_code = "forbid"` expresses "unsafe requires a recorded decision" as
"unsafe is impossible." That holds only while no platform is touched. The
current arrangement has **no floor**: workspace lints are opt-in per crate, so
deleting `[lints] workspace = true` from ten crates removes `forbid` from all
ten and **no gate fires**. Item 4 above is therefore strictly a
*strengthening* — today nothing counts, afterwards something does.

### What was measured to reach this

Five probes on a throwaway workspace, toolchain 1.88.0, edition 2024:

| Probe | Result |
|---|---|
| Inherit workspace `forbid`, then `#![allow(unsafe_code)]` at crate root | `error[E0453] ... overruled by previous forbid`; Cargo passes `-F unsafe-code` |
| `[target.'cfg(target_os = "android")'.lints.rust]` | **`warning: unused manifest key`** — Cargo silently ignores it. A target-conditional lint level does not exist, and the attempt does not even fail loudly |
| Crate declares its own `[lints.rust] unsafe_code = "deny"`, does not inherit; per-site `#[allow]`; `#[unsafe(no_mangle)] pub extern "C"` | compiles |
| A crate under `forbid` depends on and calls into an unsafe crate | compiles — lint levels stop at the crate boundary |
| Dependency reached only via `[target.'cfg(...)'.dependencies]` | never appears in a verbose host build |

The second row is why "relax the lint for Android only" is not available: the
mechanism does not exist, and asking for it produces a warning that is easy to
mistake for success.

## Measured state of the tree (2026-08-20, R1741)

### Already present — does not need building

| Item | Evidence |
|---|---|
| Render stack | winit 0.30 + wgpu 29 + vello 0.9, all with Android paths |
| Suspend/resume lifecycle | `crates/pinion-shell/src/app.rs` `resumed` / `suspended` — per-window slots cache the `Window` and drop only the GPU renderer, which is exactly what a mobile OS reclaims |
| Touch | `crates/pinion-shell/src/substrate.rs` (`TouchPhase`), 4-phase vocabulary in `pinion_core::input` |
| Socket RPC seam | `pinion-rpc-transport` + `ShellConfig::on_rpc_ingress` |
| Memory probe | reads `/proc/self/status` — works on Android (Linux kernel) |

### Blocking — structural

1. **Binary shape.** Every example is a `bin`; the tree declares no
   `crate-type` anywhere. Android loads a `cdylib`.
   Re-measure: `grep -rn "crate-type" --include=Cargo.toml crates/ examples/`
2. **No entry seam.** `run_with_config` builds the event loop directly; there
   is nowhere to pass the activity handle that
   `EventLoopBuilderExtAndroid::with_android_app` requires. winit's
   `android-native-activity` / `android-game-activity` features are also not
   defaults and must be target-gated on.
3. **`unsafe_code = "forbid"` + edition 2024** — answered by the decision above.
4. **Window model.** The shell is genuinely multi-window since R670.B
   (`resumed` walks a spec list); an activity has one surface. This gap did
   **not** exist at the R679.3 audit — it arrived with multi-window.

### Feature-integration defects — the app would not be usable

| Defect | Evidence |
|---|---|
| **CJK/Korean text entry is impossible** | winit's Android backend emits only `WindowEvent::KeyboardInput`; `Ime::` appears nowhere in it. The soft keyboard *does* open (`set_ime_allowed` calls `show_soft_input`), but composition never arrives, and pinion's whole text path runs through the `WindowEvent::Ime` arm in `pinion-shell/src/app.rs`. `set_ime_cursor_area` and `set_ime_purpose` are empty stubs, so the keyboard also cannot avoid covering the focused field. Fixing this means implementing `InputConnection` on the Java side and delivering preedit/commit over the seam — upstream does not have it. |
| **Persistence silently evaporates** | `pinion-platform-storage` resolves its root via `dirs::data_dir()`; `dirs-sys` returns `None` for `home_dir()` on Android, and Android sets neither `HOME` nor `XDG_DATA_HOME`. `FileStorage` construction then fails and the crate falls back to `InMemoryStorage` **by design** — no error, no crash, state simply does not survive a restart. The correct root is `Context.getFilesDir()`. |
| **File dialog does not compile** | `rfd 0.17.2` has backend modules for linux / gtk3 / macos / wasm / windows and none for android. Must be gated out the way `pinion-platform-clipboard` already gates arboard, then rebuilt on the Storage Access Framework. |

**A Kotlin layer is unavoidable.** winit uses `android-activity`
(NativeActivity / GameActivity), which exposes no `startActivityForResult`
equivalent. File picking, camera, sharing, runtime permissions (including
`POST_NOTIFICATIONS`), notifications and billing all require a Java-side
subclass with callbacks back into Rust. "Pure Rust app" is not available.
This is spec-legal — §3 names `External(opaque)` as an escape hatch — but it
is a second language with its own build, tests and drift risk.

### The persistence defect is a violation of an already-ratified rule

`cross-platform-native-strategy.md` rule 3 says a platform that lacks a
feature **declares it absent (queryable), never silently no-ops**. The storage
fallback is exactly the silent no-op that rule forbids; it is simply that no
platform had triggered it before. Repairing it is owed regardless of whether
Android is pursued.

## Release gates (verified against current policy, 2026-08-20)

| Gate | State |
|---|---|
| targetSdk 36 (Android 16) | Required for new apps and updates from **2026-08-31**; an extension to 2026-11-01 can be requested in Play Console |
| **Edge-to-edge enforcement** | targetSdk 36 deprecates and disables the opt-out. The tree has **no inset vocabulary at all** — re-measure: `grep -rn "safe_area\|safe_inset\|EdgeInsets\|status_bar" --include=*.rs crates/`. Every widget would draw under the status and navigation bars. **This is the one gate that blocks release outright.** |
| 16 KB page alignment | Already in force. NDK r28+ emits 16 KB-aligned libraries by default, so this is a toolchain choice, not code work |
| Predictive back | Migrate, or opt out in the manifest. The tree has no back-navigation concept |
| Large-screen orientation | Restrictions no longer apply at >= 600dp; arbitrary resize must be tolerated. The shell already handles resize |

Not blocking, confirmed: binary size (a release example measures tens of MB;
AAB splits per ABI), audio (`cpal` has an Android backend via oboe/jni/
ndk-context), accessibility (`accesskit_winit` pulls `accesskit_android` under
`cfg(target_os = "android")` — the dependency edge exists; whether it is
turnkey with `android-activity` is **unverified**).

## Toolchain note

`libxml2-dev` and `libclang-dev` remain required **on the host** even when
cross-compiling: `sce-build` is a build-dependency, its `libxml` dependency's
`build.rs` probes `pkg-config` and panics when absent, and every example
build-depends on `pinion-forge` which build-depends on `sce-build`. This is the
first thing that breaks in a clean NDK container.

## What the old audit got wrong

`mobile-target-deferred` (R679.3) is superseded on these points:

- "accesskit_winit is desktop only, TalkBack unimplemented" — **false now**;
  the Android backend is a declared dependency.
- "a mobile audio backend is needed" — **false now**; cpal covers it.
- Its cost estimate predates multi-window and the growth of the
  `pinion-platform-*` family, so the window-model and platform-crate work is
  larger than it recorded, while accessibility and audio have vanished.

## Open decisions (not settled here)

1. Is mobile admitted as a northern-star axis? The 4-phase progression is
   desktop + console throughout.
2. Is the target "runs on a real device" or "shipped on Play"? The first needs
   the toolchain, the entry seam and a one-line font-directory addition
   (`pinion-platform-fonts` answers no directories off Linux; Android's is
   `/system/fonts`); the second needs the whole list above.

The `unsafe_code` decision above is **independent of both** — it is a policy
about how the boundary opens, and costs nothing until a consumer exists.

## Next step

Build the ratchet (decision item 4) **before** any Android work, pinned at the
tree's current value. Its value is immediate and unrelated to Android: it
closes the hole described under "the invariant this does not weaken", which is
open right now. When Android arrives, the pin moves with a reason attached.

Count by AST, not by regex — this project has already recorded that an
unbounded substring match credits other people's work.
