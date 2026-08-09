//! R1617 §5.16 §2 #7 — the level-support table is checked against the window
//! backend's own source, so it cannot go stale in silence.
//!
//! # What was wrong
//!
//! [`WindowingBackend::outcome`] answers, per backend, whether a declared
//! [`WindowLevel`] is driven. That answer is a **model**, not a query: no
//! windowing system reports "I ignored that", and the window backend exposes no
//! getter for a window's level. R1610 built the model by reading the backend's
//! source once and writing the result into a `match`. Nothing then held the two
//! together — the backend gaining a Wayland implementation would leave our
//! table saying `Unsupported` forever, and neither the compiler nor any test
//! would notice. Registered as `debt-level-support-models-a-vendored-crate`,
//! and the same class as R1550's `hash_table_bytes`: model a vendored crate's
//! internals, and the number goes quietly wrong when that crate moves.
//!
//! # What this check proves, and what it does not
//!
//! It reads every `set_window_level` under the backend's `platform_impl` and
//! classifies each as a **deliberate no-op** or not, from two independent
//! signals that must agree:
//!
//! 1. the parameter binding is underscore-prefixed (Rust's own spelling for
//!    "deliberately unused", and what the compiler's unused-variable lint
//!    pushes an author towards), and
//! 2. the parameter's identifier does not appear anywhere in the body.
//!
//! When the two signals disagree the check **fails** rather than guessing: an
//! author took a level and did something odd with it, and a human should look.
//!
//! So the claim is exact in one direction and bounded in the other. Every
//! [`LevelOutcome::Unsupported`] verdict in the table is proved to correspond to
//! a deliberate no-op; every [`LevelOutcome::Applied`] verdict is proved only to
//! correspond to an implementation the level *reaches*. A body that accepted
//! the level and threw it away would satisfy this and is not a shape any of the
//! backends here writes — an intentional no-op is written `_level` in all four
//! of the ones that are one. That residual is stated rather than papered over,
//! and it sits on the side the module already chose (see its "error direction"
//! section): the check makes the silent-lie direction hard, which is the
//! direction nobody would otherwise catch.
//!
//! # Every implementation is accounted for
//!
//! The census is keyed by **source path**, and an unrecognised path fails the
//! test. That is deliberate: a version bump that moves or adds a backend is
//! exactly the moment the model needs re-reading, so it should stop the line
//! rather than quietly cover fewer files. Backends this adapter does not name
//! are listed here too, with the verdict that they must fall to
//! [`WindowingBackend::Other`] and therefore [`LevelOutcome::Unknown`] — which
//! is how the check states, rather than assumes, that under-reporting is the
//! chosen failure direction. One of them drives levels today.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pinion_core::window_level::{LevelOutcome, WindowLevel, WindowingBackend};
use quote::ToTokens;

/// What one backend's `set_window_level` does with the level it is handed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Implementation {
    /// The level reaches the body. Consistent with the backend driving it.
    Consumed,
    /// A deliberate no-op: the binding is underscore-prefixed AND unreferenced.
    DeliberateNoOp,
}

/// How this adapter classifies one source file that declares a
/// `set_window_level`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A backend this adapter names. Its implementation must match the verdict
    /// [`WindowingBackend::outcome`] gives.
    Backend(WindowingBackend),
    /// The platform-enum dispatcher, which forwards to one of the backends
    /// above rather than talking to a window system itself. It must consume the
    /// level — a dispatcher that dropped it would make every Linux verdict
    /// wrong no matter what the two implementations underneath do.
    Dispatcher,
    /// A backend this adapter does not name, so it resolves to
    /// [`WindowingBackend::Other`] and reports [`LevelOutcome::Unknown`]. The
    /// expected implementation is recorded anyway, because that is what makes
    /// the under-reporting VISIBLE: `orbital` drives levels and we say we do
    /// not know.
    Unnamed(Implementation),
}

/// Every `set_window_level` the window backend declares under `platform_impl`,
/// by path relative to its `src/`, with what this adapter expects of it.
///
/// Keyed by path on purpose. A version bump that renames or adds a file fails
/// this test, which is the point: the model is a reading of this source, so the
/// source moving is precisely when it needs re-reading. Measured against
/// 0.30.13.
const EXPECTED: &[(&str, Role)] = &[
    ("platform_impl/linux/mod.rs", Role::Dispatcher),
    (
        "platform_impl/linux/x11/window.rs",
        Role::Backend(WindowingBackend::X11),
    ),
    (
        "platform_impl/linux/wayland/window/mod.rs",
        Role::Backend(WindowingBackend::Wayland),
    ),
    (
        "platform_impl/macos/window_delegate.rs",
        Role::Backend(WindowingBackend::MacOs),
    ),
    (
        "platform_impl/windows/window.rs",
        Role::Backend(WindowingBackend::Windows),
    ),
    // The four this adapter does not name. Their expected implementations are
    // recorded so the test states the under-report rather than hiding it.
    (
        "platform_impl/android/mod.rs",
        Role::Unnamed(Implementation::DeliberateNoOp),
    ),
    (
        "platform_impl/ios/window.rs",
        Role::Unnamed(Implementation::DeliberateNoOp),
    ),
    (
        "platform_impl/web/window.rs",
        Role::Unnamed(Implementation::DeliberateNoOp),
    ),
    (
        "platform_impl/orbital/window.rs",
        Role::Unnamed(Implementation::Consumed),
    ),
];

/// The exact version of the window backend this model was read against.
///
/// Read from the workspace lock file rather than written here, so the two
/// cannot disagree — and so a bump changes one file rather than two.
fn locked_version() -> String {
    let lock = workspace_root().join("Cargo.lock");
    let text = std::fs::read_to_string(&lock)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", lock.display()));
    let mut in_package = false;
    for line in text.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            in_package = false;
            continue;
        }
        if line == r#"name = "winit""# {
            in_package = true;
            continue;
        }
        if in_package {
            if let Some(rest) = line.strip_prefix("version = \"") {
                if let Some(v) = rest.strip_suffix('"') {
                    return v.to_owned();
                }
            }
        }
    }
    panic!("the workspace lock file names no window-backend version");
}

fn workspace_root() -> PathBuf {
    // `crates/pinion-shell` -> the workspace root.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels below the workspace root")
        .to_path_buf()
}

/// The unpacked source of the locked window-backend version.
///
/// Present by construction: this test lives in the one crate that depends on
/// that backend, so a build that produced this binary also unpacked its source.
/// If it is nonetheless absent the check FAILS — a gate that quietly skips is
/// the failure mode this whole file exists to prevent.
fn backend_source() -> PathBuf {
    let version = locked_version();
    let home = std::env::var_os("CARGO_HOME").map_or_else(
        || {
            PathBuf::from(std::env::var_os("HOME").expect("neither CARGO_HOME nor HOME is set"))
                .join(".cargo")
        },
        PathBuf::from,
    );
    let registry = home.join("registry").join("src");
    let wanted = format!("winit-{version}");
    let mut tried = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&registry) {
        for entry in entries.flatten() {
            let candidate = entry.path().join(&wanted).join("src");
            if candidate.is_dir() {
                return candidate;
            }
            tried.push(candidate);
        }
    }
    panic!(
        "the locked window-backend source ({wanted}) is not unpacked under {}; \
         looked at {tried:?}. This check reads that source, so it refuses to \
         pass without it.",
        registry.display(),
    );
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Classify one `set_window_level` body.
///
/// `signature` is the function's arguments and `body` its block, both from a
/// real parse — this is an AST question, not a text one, because the answer
/// turns on which identifier is the parameter's binding and whether that exact
/// identifier occurs as a token in the body (R1527: census by parser, not by
/// regex).
fn classify(inputs: &[syn::FnArg], body: &syn::Block, at: &str) -> Implementation {
    let typed: Vec<&syn::PatType> = inputs
        .iter()
        .filter_map(|arg| match arg {
            syn::FnArg::Typed(t) => Some(t),
            syn::FnArg::Receiver(_) => None,
        })
        .collect();
    assert_eq!(
        typed.len(),
        1,
        "{at}: set_window_level should take exactly one level argument beside self",
    );
    let syn::Pat::Ident(binding) = &*typed[0].pat else {
        panic!("{at}: the level argument is not a plain binding");
    };
    let name = binding.ident.to_string();
    let underscored = name.starts_with('_');
    let referenced = mentions(body.to_token_stream(), &name);
    match (underscored, referenced) {
        (true, false) => Implementation::DeliberateNoOp,
        (false, true) => Implementation::Consumed,
        // The two signals disagree. Refusing to classify is the honest answer:
        // an author who named a parameter `_level` and then used it, or named
        // it `level` and dropped it, has written something this check cannot
        // read, and guessing would put a verdict on the wire nobody verified.
        _ => panic!(
            "{at}: cannot classify `{name}` — underscore-prefixed = {underscored}, \
             referenced in the body = {referenced}. The two signals must agree; \
             read this implementation by hand and teach the check.",
        ),
    }
}

/// Does `ident` occur as an identifier token anywhere in `stream`?
fn mentions(stream: proc_macro2::TokenStream, ident: &str) -> bool {
    stream.into_iter().any(|tree| match tree {
        proc_macro2::TokenTree::Ident(i) => i == ident,
        proc_macro2::TokenTree::Group(g) => mentions(g.stream(), ident),
        proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => false,
    })
}

/// Every `set_window_level` found, by path relative to the backend's `src/`.
fn census() -> BTreeMap<String, Implementation> {
    let src = backend_source();
    let mut files = Vec::new();
    rust_files(&src.join("platform_impl"), &mut files);
    assert!(
        !files.is_empty(),
        "no platform sources under {}",
        src.display(),
    );
    let mut found = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", file.display()));
        let Ok(parsed) = syn::parse_file(&text) else {
            // A platform source this toolchain's parser cannot read is not
            // silently skipped: it could be the one holding the answer.
            panic!("cannot parse {}", file.display());
        };
        let relative = file
            .strip_prefix(&src)
            .expect("walked from src")
            .to_string_lossy()
            .replace('\\', "/");
        for item in &parsed.items {
            collect(item, &relative, &mut found);
        }
    }
    found
}

/// Walk items looking for `fn set_window_level`, descending into inline
/// modules and impl blocks. Free functions count too — the census is about
/// which sources declare the behaviour, not about where an author put it.
fn collect(item: &syn::Item, at: &str, out: &mut BTreeMap<String, Implementation>) {
    match item {
        syn::Item::Fn(f) if f.sig.ident == "set_window_level" => {
            let inputs: Vec<syn::FnArg> = f.sig.inputs.iter().cloned().collect();
            insert(out, at, classify(&inputs, &f.block, at));
        }
        syn::Item::Impl(imp) => {
            for sub in &imp.items {
                if let syn::ImplItem::Fn(f) = sub {
                    if f.sig.ident == "set_window_level" {
                        let inputs: Vec<syn::FnArg> = f.sig.inputs.iter().cloned().collect();
                        insert(out, at, classify(&inputs, &f.block, at));
                    }
                }
            }
        }
        syn::Item::Mod(m) => {
            if let Some((_, items)) = &m.content {
                for sub in items {
                    collect(sub, at, out);
                }
            }
        }
        _ => {}
    }
}

fn insert(out: &mut BTreeMap<String, Implementation>, at: &str, found: Implementation) {
    if let Some(previous) = out.insert(at.to_owned(), found) {
        assert_eq!(
            previous, found,
            "{at} declares set_window_level twice and the two disagree",
        );
    }
}

#[test]
fn r1617_every_backend_implementation_is_accounted_for() {
    let found = census();
    let expected: BTreeMap<&str, Role> = EXPECTED.iter().copied().collect();
    let found_paths: Vec<&str> = found.keys().map(String::as_str).collect();
    let expected_paths: Vec<&str> = expected.keys().copied().collect();
    assert_eq!(
        found_paths, expected_paths,
        "the set of sources declaring set_window_level moved. That is exactly \
         when this model needs re-reading — read the new ones, update EXPECTED, \
         and re-check `WindowingBackend::outcome` against them.",
    );
}

#[test]
fn r1617_the_level_table_matches_the_backend_source() {
    let found = census();
    for (path, role) in EXPECTED {
        let implementation = *found
            .get(*path)
            .unwrap_or_else(|| panic!("{path} declares no set_window_level"));
        match role {
            Role::Dispatcher => assert_eq!(
                implementation,
                Implementation::Consumed,
                "{path} is the platform dispatcher; dropping the level there \
                 would make every verdict below it wrong",
            ),
            Role::Backend(backend) => {
                // Only a stacking request can be dropped, so this is where the
                // table's claim actually bites.
                for level in [WindowLevel::AlwaysOnTop, WindowLevel::AlwaysOnBottom] {
                    let outcome = backend.outcome(level);
                    match implementation {
                        Implementation::DeliberateNoOp => assert_eq!(
                            outcome,
                            LevelOutcome::Unsupported {
                                declared: level,
                                backend: *backend,
                            },
                            "{path} is a deliberate no-op, so {backend:?} must \
                             report {level:?} as unsupported — claiming it \
                             applied is the silent lie this axis exists to \
                             prevent",
                        ),
                        Implementation::Consumed => assert_eq!(
                            outcome,
                            LevelOutcome::Applied {
                                level,
                                backend: *backend,
                            },
                            "{path} consumes the level, so {backend:?} must not \
                             report {level:?} as unsupported — an over-report \
                             here is the self-correcting direction, but it is \
                             still wrong and now measurable",
                        ),
                    }
                }
            }
            Role::Unnamed(expected_impl) => {
                assert_eq!(
                    implementation, *expected_impl,
                    "{path} changed what it does with a level. It is unnamed by \
                     this adapter, so nothing on the wire moves — but the \
                     record of WHAT we are under-reporting must stay true.",
                );
            }
        }
    }
}

#[test]
fn r1617_the_under_report_is_real_and_named() {
    // The chosen error direction, stated as a fact rather than as a comment.
    // At least one backend this adapter does not name genuinely drives levels,
    // so `Other` -> `Unknown` is an under-report we are making on purpose, and
    // `Unknown` must therefore not read as a failure OR as a success.
    let found = census();
    let driving_unnamed: Vec<&str> = EXPECTED
        .iter()
        .filter(|(path, role)| {
            matches!(role, Role::Unnamed(Implementation::Consumed))
                && found.get(*path) == Some(&Implementation::Consumed)
        })
        .map(|(path, _)| *path)
        .collect();
    assert!(
        !driving_unnamed.is_empty(),
        "if no unnamed backend drives a level any more, the under-report this \
         adapter documents has become hypothetical — say so rather than keep \
         claiming it",
    );
    for level in [WindowLevel::AlwaysOnTop, WindowLevel::AlwaysOnBottom] {
        let outcome = WindowingBackend::Other.outcome(level);
        assert_eq!(outcome.kind(), "unknown");
        assert!(
            !outcome.is_honoured(),
            "an unmeasured backend must not read as a success",
        );
        assert_eq!(
            outcome.declared(),
            level,
            "nor lose the declaration on the way",
        );
    }
}

#[test]
fn r1617_the_classifier_reads_the_ast_not_the_text() {
    // The classifier's own discrimination, on fixtures rather than on the
    // vendored source — so a change to that source cannot quietly make these
    // two cases stop being tested.
    let no_op: syn::ImplItemFn =
        syn::parse_quote! { pub fn set_window_level(&self, _level: WindowLevel) {} };
    let inputs: Vec<syn::FnArg> = no_op.sig.inputs.iter().cloned().collect();
    assert_eq!(
        classify(&inputs, &no_op.block, "fixture"),
        Implementation::DeliberateNoOp,
    );

    let driving: syn::ImplItemFn = syn::parse_quote! {
        pub fn set_window_level(&self, level: WindowLevel) {
            self.set_flag(level == WindowLevel::AlwaysOnTop);
        }
    };
    let inputs: Vec<syn::FnArg> = driving.sig.inputs.iter().cloned().collect();
    assert_eq!(
        classify(&inputs, &driving.block, "fixture"),
        Implementation::Consumed,
    );

    // A body that mentions the level only inside a macro argument still counts
    // — token groups are descended, which a top-level-tokens-only scan would
    // miss and report as a no-op.
    let nested: syn::ImplItemFn = syn::parse_quote! {
        pub fn set_window_level(&self, level: WindowLevel) {
            tracing::debug!(?level, "setting");
        }
    };
    let inputs: Vec<syn::FnArg> = nested.sig.inputs.iter().cloned().collect();
    assert_eq!(
        classify(&inputs, &nested.block, "fixture"),
        Implementation::Consumed,
    );

    // And a name that merely CONTAINS the parameter's spelling is not a
    // reference to it — the difference between an identifier token and a
    // substring, which is what makes this a parse rather than a grep (R1560's
    // unbounded-substring finding).
    let lookalike: syn::ImplItemFn = syn::parse_quote! {
        pub fn set_window_level(&self, _level: WindowLevel) {
            self.reset_level_cache();
        }
    };
    let inputs: Vec<syn::FnArg> = lookalike.sig.inputs.iter().cloned().collect();
    assert_eq!(
        classify(&inputs, &lookalike.block, "fixture"),
        Implementation::DeliberateNoOp,
        "`reset_level_cache` contains `level` and is not the parameter",
    );
}

#[test]
#[should_panic(expected = "cannot classify")]
fn r1617_disagreeing_signals_stop_the_line_rather_than_guess() {
    // An author who names the binding `level` and then drops it has written a
    // shape this check cannot read. Guessing either way would put an unverified
    // verdict on the wire, so it refuses.
    let odd: syn::ImplItemFn = syn::parse_quote! {
        pub fn set_window_level(&self, level: WindowLevel) {}
    };
    let inputs: Vec<syn::FnArg> = odd.sig.inputs.iter().cloned().collect();
    let _ = classify(&inputs, &odd.block, "fixture");
}
