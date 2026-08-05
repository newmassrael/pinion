//! R641 §5.16 — `#[pinion::widget]` attribute macro.
//!
//! Lifts the mechanical wiring of the `WidgetCore` / `WidgetA11y` /
//! `WidgetView` supertrait chain into a single attribute attached to
//! the widget unit struct, leaving the widget-specific logic
//! (`read_state` / `event_name` / `view`) as inherent methods the macro
//! forwards into. R642 §5.16 added the declarative `role` +
//! `state_flags(...)` attributes that auto-derive the single-node
//! `WidgetA11y::access_node` body for the 80 % case (`Button`-shaped
//! widgets — see [`crate::widget`](mod@crate::widget) module docs for the variant table),
//! leaving the inherent fn path open as the escape hatch for
//! composite widgets (`RadioGroup`, `Listbox`).
//!
//! ## Why an attribute macro, not three derives
//!
//! The three traits are intentionally split across crates so a TUI
//! binding can replace `WidgetView` (Vello-bound) with
//! `WidgetViewTui` (ratatui-bound) without losing the shared
//! `WidgetCore` + `WidgetA11y` surface. Three separate `#[derive(...)]`
//! macros would force the author to repeat the binding identity
//! (`tag` / `state` / `event` / `external` / `title`) on each derive
//! attribute. One attribute on the struct collapses that surface and
//! mirrors the axum / structopt convention of "one attribute carries
//! the whole binding metadata".
//!
//! Rust trait impls cannot be split across multiple `impl Trait for X`
//! blocks, so the macro emits the **full** trait impl for each of the
//! three traits in one shot. Methods with widget-specific logic
//! forward to inherent `fn` items on the unit struct; the rest stay at
//! their `WidgetCore` / `WidgetA11y` trait-level defaults unless the
//! author opts into forwarding via a flag attribute.
//!
//! ## Required attributes
//!
//! - `tag = "main_btn"` — paint-side dispatch tag the `InputRouter`
//!   hit-tests against. Returned from `WidgetCore::tag`.
//! - `state = ButtonState` — `WidgetCore::State` associated type.
//! - `event = ButtonEvent` — `WidgetCore::Event` associated type.
//! - `title = "Hello Button"` — OS window title. Returned from
//!   `WidgetCore::title`.
//! - `renderer = HelloButtonRenderer` — `WidgetView::Renderer`
//!   associated type (pinion-forge-emitted Vello renderer struct).
//! - `initial_size = (W, H)` — logical-pixel default window size.
//!   Returned from `WidgetView::initial_size`.
//! - `external = ButtonExternal::new` — factory expression invoked
//!   inside `WidgetCore::create_external` (`Box::new(<expr>())`).
//! - `extra_externals = make_inspector_extras` (R1249, optional) —
//!   nullary factory whose `Vec<ExtraExternal>` the emitted
//!   `WidgetCore::create_extra_externals` returns, letting a macro
//!   binding host sibling `External`s (an inline `TextField`, a
//!   scrollbar) without dropping to a hand-written `impl WidgetCore`.
//!   Incompatible with `read_state_derive` / `state_name_derive` (those
//!   match `Scene::External` directly and cannot walk the `Container`
//!   shape extras impose — rejected at parse time); the binding supplies
//!   an inherent `read_state` calling `Scene::find_external_with_tag`.
//!
//! ## Required inherent methods
//!
//! The macro emits forwarding stubs for these three methods, which
//! every widget binding has widget-specific logic for and which have
//! no sensible default at the trait level:
//!
//! ```rust,ignore
//! impl ButtonView {
//!     fn view(state: ButtonState, frame: Frame) -> Scene { ... }
//!     fn read_state(scene: &Scene) -> ButtonState { ... }
//!     fn event_name(event: ButtonEvent) -> &'static str { ... }
//! }
//! ```
//!
//! The macro emits `<ButtonView>::view(state, *frame)` etc. as the
//! trait body — if the inherent fn is missing the compile error
//! surfaces at the trait impl site with the standard "no function
//! named `view` found" message.
//!
//! `access_node` is the fourth method with no sensible trait default
//! but R642 §5.16 added two paths:
//!
//! - **Declarative path (80 %)** — supply `role = <AriaRole>` plus an
//!   optional `state_flags(...)` clause; the macro derives a
//!   single-node `WidgetA11y::access_node` body that matches the
//!   `Button`-shaped widgets surveyed in the R642 audit
//!   (`hello-button` / `figma-button-m3` / future `Slider` /
//!   `TextInput`). No inherent `fn access_node` is required when the
//!   declarative path is taken.
//! - **Inherent path (escape hatch)** — omit `role` and provide
//!   `fn access_node(state: <State>, focused: Option<&str>) -> Vec<AccessNode>`
//!   on the unit struct. The macro forwards into it unchanged
//!   (R641 behaviour). Composite widgets (`RadioGroup` / `Listbox`)
//!   stay on the inherent path because their multi-node enumeration
//!   doesn't fit the single-node `AccessState` shape.
//!
//! ## Optional `role` / `state_flags` derive (R642)
//!
//! `role` selects the `AriaRole` variant the derived node carries
//! (`Button`, `Switch`, `CheckBox`, `Slider`, `TextInput`,
//! `RadioButton` — single-node roles).
//!
//! `state_flags(...)` declares the state-variant → `AccessState`
//! bool-flag mapping. Each entry is `flag = Variant` where `flag` is
//! one of `hovered` / `pressed` / `disabled` / `checked` / `expanded`
//! and `Variant` is a bare unit-variant ident of `Self::State`:
//!
//! | Flag       | Maps to                              | Variant absent → |
//! | ---------- | ------------------------------------ | ---------------- |
//! | `hovered`  | `matches!(state, State::Variant)`    | `false`          |
//! | `pressed`  | `matches!(state, State::Variant)`    | `false`          |
//! | `disabled` | `matches!(state, State::Variant)`    | `false`          |
//! | `checked`  | `Some(matches!(state, State::Variant))` _or_ `Some(state.N)` | `None`        |
//! | `expanded` | `Some(matches!(state, State::Variant))` _or_ `Some(state.N)` | `None`        |
//!
//! R696 §5.16 — `expanded` (WAI-ARIA `aria-expanded`) accepts the same
//! two forms as `checked` and maps to `AccessNode::expanded`. The
//! disclosure / accordion widget (`(DisclosureState, bool)`) is the
//! first consumer, declared as `expanded = bool_field(1)`. It is a
//! distinct a11y axis from `checked`: a disclosure header is a
//! `button` reporting whether a *separate* region is shown, not a
//! `checkbox` reporting its own on/off value.
//!
//! `focused` is auto-derived from `focused == Some(<Self as WidgetCore>::tag())`
//! for every declarative node — no `state_flags` entry needed.
//!
//! R653 §5.16 — `checked` accepts a second form `bool_field(N)` to
//! support tuple-state widgets `(EnumState, bool)` (R51.4 Toggle /
//! R51.5 Checkbox / R51.6 Radio / R57.X.toggle hello-theme) where the
//! on/off semantic is carried by the second tuple element (a
//! `bool`), not by an enum variant. `checked = bool_field(1)` emits
//! `Some(state.1)` directly. The enum-variant form
//! (`checked = OnVariant`) is unchanged and remains the only form
//! `hovered` / `pressed` / `disabled` accept (those three are always
//! interaction-state-enum dispatched).
//!
//! Multi-field state structs (`CheckboxState { ..., checked: bool }`)
//! still aren't reachable from `state_flags` v0 — those bindings stay
//! on the inherent path until a future macro round teaches the parser
//! the field-access form (`checked = field(checked)`). R642 covers the
//! enum-variant shape (`ButtonState::Hover`); R645 lifts that to
//! `(EnumState, _)` tuple first-elem dispatch for `hovered` /
//! `pressed` / `disabled`; R653 extends `checked` with the second-
//! elem bool extraction so the Cat A Toggle / Checkbox / Radio /
//! Theme retrofit cascade (R652 ROI matrix) lands without manual
//! `WidgetA11y` impls.
//!
//! ## Optional forward flags
//!
//! Flag-style attributes opt into forwarding additional methods to
//! inherent `fn` items the author provides; absent the flag, the
//! trait default applies. R642 §5.16 ([[abstraction-needs-second-consumer]])
//! pruned the R641 optional list down to the two flags `hello-button`
//! actually consumes (`apply_key` + `keybinding`); the macro accepts
//! no other flags. Future text / composite retrofits add their flag
//! back to the macro at the round that materialises the second
//! consumer:
//!
//! | Flag                 | Trait default (no flag)               | When to opt in                                                                                                |
//! | -------------------- | ------------------------------------- | ------------------------------------------------------------------------------------------------------------- |
//! | `apply_key`          | `false` (no key handled)              | ARIA Space/Enter activation, Arrow keys, custom keys                                                          |
//! | `keybinding`         | `None` (no character-key mapping)     | Single-char shortcuts mapping to `Self::Event`                                                                |
//! | `state_name_derive`  | forward to inherent `fn read_state` + `fn event_name` (R641) | Enum-shaped `Self::State` + `Self::Event` with `WidgetStateName` + `WidgetEventName` impls (R643) |
//! | `update`             | `Vec::new()` (no reducer side effects) | R27 reducer that turns SCXML intents into `Command`s — Toggle / Theme flip the palette here (R653)             |
//!
//! ## Example
//!
//! ```rust,ignore
//! use pinion_derive::widget;
//!
//! #[widget(
//!     tag = "main_btn",
//!     state = ButtonState,
//!     event = ButtonEvent,
//!     title = "hello-button",
//!     renderer = HelloButtonRenderer,
//!     initial_size = (320, 200),
//!     external = ButtonExternal::new,
//!     role = Button,
//!     state_flags(
//!         hovered = Hover,
//!         pressed = Pressed,
//!         disabled = Disabled,
//!     ),
//!     apply_key,
//! )]
//! struct ButtonView;
//!
//! impl ButtonView {
//!     fn view(state: ButtonState, frame: Frame) -> Scene { /* ... */ }
//!     fn read_state(scene: &Scene) -> ButtonState { /* ... */ }
//!     fn event_name(event: ButtonEvent) -> &'static str { /* ... */ }
//!     fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str,
//!         modifiers: Modifiers) -> bool { /* ... */ }
//! }
//! ```

use std::collections::HashSet;

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Expr, ExprPath, Ident, ItemStruct, LitStr, Token, Type,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
};

/// R1570 §5.16 — wrap a forward-to-inherent call in the guard that makes a
/// MISSING inherent function a compile error instead of infinite recursion.
///
/// Every body this macro generates that reads `<TheView>::name(..)` is calling
/// a function it also *defines*, one trait level up. When the view declares the
/// inherent function, that one wins and the forward is a bridge. When it does
/// not, the call resolves back to the trait method it sits inside and the
/// forward calls itself — which release-mode tail-call optimisation turns into
/// a bare jump: 100% CPU, no stack growth, no syscall, never returns.
///
/// Bringing [`pinion_core::widget_forward`]'s guard for that one name into
/// scope — for the length of this body and nothing more — gives the call a
/// second candidate in exactly the case where the inherent function is absent,
/// so rustc reports [E0034] at the `#[widget(...)]` attribute and names the
/// guard. See that module for why one guard per name, and why their signatures
/// need not match the forwarded ones.
///
/// [`pinion_core::widget_forward`]: ../pinion_core/widget_forward/index.html
/// [E0034]: https://doc.rust-lang.org/error_codes/E0034.html
fn guarded_forward(guard: &str, call: &TokenStream2) -> TokenStream2 {
    let guard = Ident::new(guard, Span::call_site());
    quote! {
        {
            // Anonymous: the guard is a resolution candidate, never a name the
            // generated body or the binding's own code can see.
            #[allow(unused_imports)]
            use ::pinion_core::widget_forward::#guard as _;
            #call
        }
    }
}

/// R644 §5.16 — source form of the `tag = X` attribute.
///
/// `LitStr` keeps the R641 literal form (`tag = "main_btn"`) so
/// bindings that have not adopted the `WidgetTag`
/// derive yet compile unchanged. `Path` accepts an enum variant
/// (`tag = Tags::MainBtn`) and emits `<Path as ::pinion_core::WidgetTag>::as_tag(&Path)`
/// as the `WidgetCore::tag` body so
/// every site references the single source of truth.
enum TagSource {
    Lit(LitStr),
    Path(ExprPath),
}

/// Entry point for [`crate::widget`](macro@crate::widget). Parses the attribute and the
/// item, then assembles the three forwarding trait impls.
pub(crate) fn expand(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let args: WidgetArgs = syn::parse2(attr)?;
    let item: ItemStruct = syn::parse2(item)?;
    let view_ident = &item.ident;

    let WidgetArgs {
        tag,
        state,
        event,
        title,
        renderer,
        initial_size: (init_w, init_h),
        external,
        extra_externals,
        role,
        apply_key_strategy,
        state_flags,
        access_value,
        flags,
    } = args;

    let tag_body = match &tag {
        TagSource::Lit(lit) => quote! { #lit },
        TagSource::Path(path) => quote! {
            // R644 §5.16 — `#path` is an expression position (the
            // enum variant, e.g. `Tags::MainBtn`), so we cannot use
            // `<#path as Trait>::method(...)` qualified syntax
            // (that wants `<#path>` to be a *type* — `MainBtn` is
            // a variant, not a type). Trait-method-call form lets
            // the compiler infer the carrier type from the
            // borrowed instance.
            ::pinion_core::WidgetTag::as_tag(&#path)
        },
    };

    let optional_forwards = emit_optional_forwards(
        view_ident,
        &state,
        &event,
        &flags,
        apply_key_strategy.as_ref(),
    );
    // R645 §5.16 — `a11y_manual` flag suppresses the WidgetA11y emit
    // entirely (Slider needs `.with_value(AccessValue::Float)` past
    // the macro's `state_flags`-only derive; ImageButton or other
    // value-bearing widgets would join that path). Without the flag
    // the macro emits either the R642 derived form (role present)
    // or the R641 forwarding form (role absent).
    let a11y_impl = if flags.contains("a11y_manual") {
        TokenStream2::new()
    } else {
        emit_a11y_impl(
            view_ident,
            &state,
            role.as_ref(),
            &state_flags,
            access_value.as_ref(),
        )
    };
    let CoreMethodBodies {
        read_state: read_state_body,
        event_name: event_name_body,
        view: view_body,
    } = emit_core_method_bodies(view_ident, &state, &event, &flags);
    let initial_size_strategy_body =
        emit_initial_size_strategy_body(view_ident, &init_w, &init_h, &flags);

    let extra_externals_impl = emit_extra_externals(extra_externals.as_ref());

    Ok(quote! {
        #item

        impl ::pinion_core::WidgetCore for #view_ident {
            type State = #state;
            type Event = #event;

            fn tag() -> &'static str { #tag_body }
            fn title() -> &'static str { #title }

            fn create_external() -> ::std::boxed::Box<dyn ::pinion_core::external::External> {
                ::std::boxed::Box::new(#external())
            }

            #extra_externals_impl

            fn read_state(scene: &::pinion_core::Scene) -> #state {
                #read_state_body
            }

            fn event_name(event: #event) -> &'static str {
                #event_name_body
            }

            fn view(state: #state, frame: &::pinion_core::Frame) -> ::pinion_core::Scene {
                // R641 §5.16 — bridge trait by-ref (`&Frame`) to the
                // by-value inherent fn signature the workspace
                // `clippy::pedantic` `trivially_copy_pass_by_ref` lint
                // prefers (`Frame` is `Copy` + 4 bytes today). Macro
                // is the boundary; user code stays clippy-clean.
                #view_body
            }

            #optional_forwards
        }

        #a11y_impl

        impl ::pinion_shell::WidgetView for #view_ident {
            type Renderer = #renderer;
            fn initial_size_strategy() -> ::pinion_shell::SizeStrategy {
                #initial_size_strategy_body
            }
        }
    })
}

/// R641 §5.16 — the bodies of the three [`pinion_core::WidgetCore`] methods the
/// macro emits for EVERY binding, as opposed to the flag-gated set
/// [`emit_optional_forwards`] handles.
///
/// R1570 lifted them out of [`expand`] as a group, because they share one
/// decision — derive the body, or forward to the view's own inherent function —
/// and because that decision is now the thing worth reading in one place: each
/// forward carries a [`guarded_forward`] guard, and each derive does not need
/// one. `view` is in the group despite never deriving, since it is the third
/// always-emitted method and separating it would put two of three here.
///
/// [`pinion_core::WidgetCore`]: ../pinion_core/trait.WidgetCore.html
struct CoreMethodBodies {
    read_state: TokenStream2,
    event_name: TokenStream2,
    view: TokenStream2,
}

fn emit_core_method_bodies(
    view_ident: &Ident,
    state: &Type,
    event: &Type,
    flags: &HashSet<String>,
) -> CoreMethodBodies {
    // R645 §5.16 — derive flags split. `state_name_derive` was the
    // R643 combined flag; R645 split it so tuple-state bindings
    // (`Slider` / `TextField` / `Toggle` — first elem enum + second
    // elem value field) can use `event_name_derive` alone without
    // claiming `read_state_derive` they cannot satisfy (tuple
    // `read_state` reads two introspect fields; `WidgetStateName`
    // only covers the enum half today). `state_name_derive` is
    // still accepted as an alias for the combined R643 form to
    // keep R642/R643 retrofit bindings working unchanged.
    let combined = flags.contains("state_name_derive");
    let derive_read_state = combined || flags.contains("read_state_derive");
    let derive_event_name = combined || flags.contains("event_name_derive");
    let read_state = if derive_read_state {
        // R643 §5.16 — derive via WidgetStateName trait. Walks the
        // same introspect chain every binding hand-wrote pre-R643
        // (Scene::External → ExternalIntrospect → query("state") →
        // Text); a missing field at any link falls back to the
        // default variant (matches the pre-R643 `_ => Self::Idle`
        // convention via `from_name_or_default("")`).
        quote! {
            if let ::pinion_core::Scene::External(node) = scene {
                if let ::core::option::Option::Some(intro) = node.handle.introspect() {
                    if let ::core::option::Option::Some(
                        ::pinion_core::external::IntrospectValue::Text(name)
                    ) = intro.query("state") {
                        return <#state as ::pinion_core::WidgetStateName>
                            ::from_name_or_default(&name);
                    }
                }
            }
            <#state as ::pinion_core::WidgetStateName>::from_name_or_default("")
        }
    } else {
        guarded_forward(
            "ReadStateNeedsInherentFn",
            &quote! { <#view_ident>::read_state(scene) },
        )
    };
    let event_name = if derive_event_name {
        // R643 §5.16 — derive via WidgetEventName trait. Every variant
        // (including the SCXML 3.13 Null sentinel + internal-only
        // ones the winit handler never produces) gets its canonical
        // PascalCase name; the pre-R643 `_ => "__internal__"` catch-
        // all is gone (every binding's `parse_X_event` rejected it
        // anyway — losing nothing, gaining AI-side observability
        // for the previously-hidden variants).
        quote! { <#event as ::pinion_core::WidgetEventName>::as_name(&event) }
    } else {
        guarded_forward(
            "EventNameNeedsInherentFn",
            &quote! { <#view_ident>::event_name(event) },
        )
    };
    let view = guarded_forward(
        "ViewNeedsInherentFn",
        &quote! { <#view_ident>::view(state, *frame) },
    );

    CoreMethodBodies {
        read_state,
        event_name,
        view,
    }
}

/// R670 §5.16 — `initial_size_strategy` body selector. The default emit is
/// `SizeStrategy::Fixed { width, height }` from the `initial_size = (W, H)`
/// attribute pair (the policy every R668 binding migrated to). The
/// `initial_size_strategy` flag opts into forwarding to the binding's inherent
/// fn instead, used by `hello-popover` (and any future binding) that needs
/// `SizeStrategy::IntrinsicAfterFirstPaint` or a custom variant the macro
/// doesn't know how to emit declaratively.
///
/// The `initial_size = (W, H)` attribute remains mandatory even when the
/// strategy is overridden — the values feed `WidgetView::initial_size` (the
/// cell-unit fallback the TUI sibling and headless screenshot path both still
/// consult) and anchor winit's `set_min_inner_size` clamp at the same floor
/// the strategy's `min` declares.
fn emit_initial_size_strategy_body(
    view_ident: &Ident,
    init_w: &Expr,
    init_h: &Expr,
    flags: &HashSet<String>,
) -> TokenStream2 {
    if flags.contains("initial_size_strategy") {
        guarded_forward(
            "InitialSizeStrategyNeedsInherentFn",
            &quote! { <#view_ident>::initial_size_strategy() },
        )
    } else {
        quote! {
            ::pinion_shell::SizeStrategy::Fixed {
                width: #init_w,
                height: #init_h,
            }
        }
    }
}

/// R1249 §5.45 — optional `create_extra_externals` override. Present only
/// when the binding declared `extra_externals = <factory>`; the factory is a
/// nullary fn returning `Vec<ExtraExternal>` invoked inside the root Owner
/// scope (so `use_*` reactive hooks in it resolve the same `Rc`s the view fn
/// later reads). Absent → the trait default (empty) stands and the
/// R1570.2 §5.16 — the `apply_key` body a `<strategy>` names, derived.
///
/// `aria_activate` is `apply_aria_activate(scene, focused, key, Self::tag())`:
/// Space / Enter activate the control when it holds focus, which is what
/// WAI-ARIA asks of every operable role and what HTML and Qt both give without
/// being asked. R1570.2's census found **25** bindings writing exactly that,
/// byte for byte — mechanical wiring with no opinion in it, which is the
/// [[three-site-internal-duplication-substrate-lift]] shape at eight times the
/// threshold.
///
/// Emitted as the whole trait method rather than as a body handed to the
/// forwarding arm, because the two are alternatives: this one needs no
/// inherent fn and therefore no [`guarded_forward`] guard. The guard exists to
/// catch a forward that resolves to itself, and there is no forward here.
///
/// `tag()` is reached through the qualified `<View as WidgetCore>::tag()`
/// rather than `Self::tag()`, so the emitted body does not depend on which
/// trait `Self` resolves through at the expansion site.
fn emit_derived_apply_key(view_ident: &Ident, strategy: &Ident) -> TokenStream2 {
    debug_assert_eq!(strategy.to_string(), "aria_activate", "parser gates this");
    quote! {
        fn apply_key(
            scene: &mut ::pinion_core::Scene,
            focused: ::core::option::Option<&str>,
            key: &str,
            modifiers: ::pinion_core::input::Modifiers,
        ) -> bool {
            let _ = modifiers;
            ::pinion_core::widgets::aria::apply_aria_activate(
                scene,
                focused,
                key,
                <#view_ident as ::pinion_core::WidgetCore>::tag(),
            )
        }
    }
}

/// single-External state-scene shape is preserved bit-for-bit.
fn emit_extra_externals(extra_externals: Option<&Expr>) -> TokenStream2 {
    match extra_externals {
        Some(factory) => quote! {
            fn create_extra_externals(
            ) -> ::std::vec::Vec<::pinion_core::widget_core::ExtraExternal> {
                #factory()
            }
        },
        None => TokenStream2::new(),
    }
}

fn emit_optional_forwards(
    view_ident: &Ident,
    state: &Type,
    event: &Type,
    flags: &HashSet<String>,
    apply_key_strategy: Option<&Ident>,
) -> TokenStream2 {
    let mut out = TokenStream2::new();
    if let Some(strategy) = apply_key_strategy {
        out.extend(emit_derived_apply_key(view_ident, strategy));
    }
    if flags.contains("apply_key") {
        let body = guarded_forward(
            "ApplyKeyNeedsInherentFn",
            &quote! { <#view_ident>::apply_key(scene, focused, key, modifiers) },
        );
        out.extend(quote! {
            fn apply_key(
                scene: &mut ::pinion_core::Scene,
                focused: ::core::option::Option<&str>,
                key: &str,
                modifiers: ::pinion_core::input::Modifiers,
            ) -> bool {
                #body
            }
        });
    }
    if flags.contains("keybinding") {
        let body = guarded_forward(
            "KeybindingNeedsInherentFn",
            &quote! { <#view_ident>::keybinding(key) },
        );
        out.extend(quote! {
            fn keybinding(key: &str) -> ::core::option::Option<#event> {
                #body
            }
        });
    }
    if flags.contains("fmt_state_log") {
        // R645 §5.16 — restore the R642.A-pruned flag. Slider is the
        // 2nd consumer (`hello-toggle` was the R641-era 1st); the
        // [[abstraction-needs-second-consumer]] gate now passes.
        //
        // [[widget-macro-by-value-bridge]] — trait method is `&Self::State`
        // but small Copy state ((Enum, f32) = 8 bytes) trips
        // `clippy::trivially_copy_pass_by_ref` on the inherent fn.
        // Macro absorbs the deref + the inherent takes by-value so
        // user code stays clippy-clean.
        let body = guarded_forward(
            "FmtStateLogNeedsInherentFn",
            &quote! { <#view_ident>::fmt_state_log(*state) },
        );
        out.extend(quote! {
            fn fmt_state_log(state: &#state) -> ::std::string::String {
                #body
            }
        });
    }
    if flags.contains("update") {
        // R653 §5.16 — forward the R27 reducer side-effect hook
        // (`WidgetCore::update`). Trait signature takes State by value
        // (Copy bound on `Self::State` guarantees no borrow contention
        // with the in-flight SCXML transition observed via read_state).
        // The inherent body owns the Intent dispatch + Vec<Command>
        // return; the macro just bridges the trait method.
        //
        // First consumer = `hello-toggle` / `hello-theme` (R57.X theme
        // palette flip on toggle intent — authority comes from
        // intent.payload, NOT V::read_state which lags the in-flight
        // SCXML transition by one tick per
        // [[intent-payload-post-flip-authority]]).
        let body = guarded_forward(
            "UpdateNeedsInherentFn",
            &quote! { <#view_ident>::update(state, intent) },
        );
        out.extend(quote! {
            fn update(
                state: #state,
                intent: &::pinion_core::intent::Intent,
            ) -> ::std::vec::Vec<::pinion_core::command::Command> {
                #body
            }
        });
    }
    out
}

/// R642 §5.16 — emit the `WidgetA11y` impl. Two paths:
///
/// - When `role` is supplied → emit a derived single-node body using
///   the declarative `state_flags(...)` mapping (80 % case). No
///   inherent `fn access_node` required on the unit struct. R645
///   extended this to tuple state types `(StateEnum, ValueT)` —
///   the macro auto-extracts the first tuple element as the enum
///   to match against, so `Slider` / `TextField` / `Toggle`
///   class widgets get the same declarative form without manual
///   tuple-destructuring boilerplate.
/// - When `role` is absent → forward to `<#view_ident>::access_node`,
///   preserving R641 behaviour for composite widgets that need the
///   inherent escape hatch (`RadioGroup`, `Listbox`).
fn emit_a11y_impl(
    view_ident: &Ident,
    state: &Type,
    role: Option<&Ident>,
    state_flags: &StateFlagsConfig,
    access_value: Option<&StateFlagSource>,
) -> TokenStream2 {
    if let Some(role_ident) = role {
        // R645 §5.16 — tuple state `(SliderState, f32)` etc. dispatches
        // bool flags on the first element. Non-tuple state matches the
        // value directly (R642 behaviour).
        let (match_target, enum_type) = match state {
            Type::Tuple(tup) if !tup.elems.is_empty() => {
                let first = tup.elems.first().expect("non-empty per match guard");
                (quote! { state.0 }, quote! { #first })
            }
            _ => (quote! { state }, quote! { #state }),
        };
        let bool_flag = |v: &Option<Ident>| {
            v.as_ref().map_or_else(
                || quote! { false },
                |variant| quote! { ::core::matches!(#match_target, #enum_type::#variant) },
            )
        };
        let hovered_expr = bool_flag(&state_flags.hovered);
        let pressed_expr = bool_flag(&state_flags.pressed);
        let disabled_expr = bool_flag(&state_flags.disabled);
        let checked_expr = state_flags.checked.as_ref().map_or_else(
            || quote! { ::core::option::Option::None },
            |source| match source {
                StateFlagSource::Variant(v) => quote! {
                    ::core::option::Option::Some(
                        ::core::matches!(#match_target, #enum_type::#v)
                    )
                },
                // R653 §5.16 — tuple-state second-elem direct bool
                // extraction. `state.N` (N must be < tuple arity, parser
                // accepted the literal). For `(EnumState, bool)` the
                // canonical use is `checked = bool_field(1)`.
                StateFlagSource::BoolField(idx) => {
                    let idx_lit = syn::Index::from(*idx);
                    quote! { ::core::option::Option::Some(state.#idx_lit) }
                }
            },
        );
        // R696 §5.16 — optional `.with_expanded(bool)` chain for the
        // WAI-ARIA `aria-expanded` axis (disclosure controls). Like
        // `value_chain`, this attaches to the derived `AccessNode`
        // builder rather than the `AccessState` literal — `expanded` is
        // an `AccessNode` field (mirror `selected` / `modal`), so the
        // additive axis stays off the `AccessState` struct the
        // hand-written `a11y_manual` bindings construct by literal.
        // Maps the declared source (enum-variant or tuple
        // `bool_field(N)`) to a bare `bool`; absent → no chain so
        // non-disclosure widgets keep the attribute omitted.
        let expanded_chain = match &state_flags.expanded {
            None => TokenStream2::new(),
            Some(StateFlagSource::Variant(v)) => quote! {
                .with_expanded(::core::matches!(#match_target, #enum_type::#v))
            },
            Some(StateFlagSource::BoolField(idx)) => {
                let idx_lit = syn::Index::from(*idx);
                quote! { .with_expanded(state.#idx_lit) }
            }
        };

        // R653 §5.16 — optional `.with_value(AccessValue::Bool(state.N))`
        // chain for value-bearing ARIA roles (Switch / CheckBox /
        // RadioButton). Slotted between `AccessNode::new(...)` and
        // `.with_state(...)` to mirror the hand-written body order
        // every Cat A binding (R652 ROI matrix) used pre-retrofit.
        let value_chain = match access_value {
            None => TokenStream2::new(),
            Some(StateFlagSource::BoolField(idx)) => {
                let idx_lit = syn::Index::from(*idx);
                quote! {
                    .with_value(::pinion_a11y::AccessValue::Bool(state.#idx_lit))
                }
            }
            Some(StateFlagSource::Variant(_)) => {
                // Defended at parse time (`access_value = bool_field(N)`
                // is the only accepted form), so this branch is
                // unreachable in well-formed input. Emit a compile-time
                // error rather than swallowing it silently.
                quote! {
                    ::std::compile_error!(
                        "internal: `access_value = Variant` form reached emit; \
                         parser should have rejected at the call site"
                    )
                }
            }
        };

        quote! {
            impl ::pinion_a11y::WidgetA11y for #view_ident {
                fn access_node(
                    state: &#state,
                    focused: ::core::option::Option<&str>,
                ) -> ::std::vec::Vec<::pinion_a11y::AccessNode> {
                    // R642 §5.16 — single-node derive. `focused`
                    // matches the standard idiom every binding
                    // hand-wrote pre-R642 (`focused == Some(tag())`).
                    let access_state = ::pinion_a11y::AccessState {
                        focused: focused == ::core::option::Option::Some(
                            <#view_ident as ::pinion_core::WidgetCore>::tag(),
                        ),
                        disabled: #disabled_expr,
                        hovered: #hovered_expr,
                        pressed: #pressed_expr,
                        checked: #checked_expr,
                        // R1229 — the derive never marks a widget indeterminate;
                        // a tri-state checkbox opts in via `with_mixed`.
                        ..::pinion_a11y::AccessState::default()
                    };
                    ::std::vec![
                        ::pinion_a11y::AccessNode::new(
                            <#view_ident as ::pinion_core::WidgetCore>::tag(),
                            ::pinion_a11y::AriaRole::#role_ident,
                        )
                        #value_chain
                        #expanded_chain
                        .with_state(access_state)
                    ]
                }
            }
        }
    } else {
        emit_a11y_forwarding_impl(view_ident, state)
    }
}

/// R641 §5.16 — the `WidgetA11y` impl for a binding that supplies its own
/// `access_node` (no `role` declared): the escape hatch composite widgets
/// (`RadioGroup` / `Listbox`) take, whose multi-node enumeration does not fit
/// the single-node derive.
///
/// R1570 lifted it out of [`emit_a11y_impl`], whose two arms have nothing in
/// common but the trait they implement — one derives a node from declarations,
/// this one forwards — and which the arm's own guard pushed past the length
/// limit. That limit was pointing at the right seam.
fn emit_a11y_forwarding_impl(view_ident: &Ident, state: &Type) -> TokenStream2 {
    let body = guarded_forward(
        "AccessNodeNeedsInherentFn",
        // R641 §5.16 — bridge trait `&State` to the inherent by-value
        // signature `WidgetCore::State: Copy` guarantees is always sound +
        // matches the clippy `trivially_copy_pass_by_ref` preference for
        // Copy ZST / one-byte enums.
        &quote! { <#view_ident>::access_node(*state, focused) },
    );
    quote! {
        impl ::pinion_a11y::WidgetA11y for #view_ident {
            fn access_node(
                state: &#state,
                focused: ::core::option::Option<&str>,
            ) -> ::std::vec::Vec<::pinion_a11y::AccessNode> {
                #body
            }
        }
    }
}

/// (R653 §5.16) Source form of a `state_flags(checked = ...)` value.
///
/// `Variant` covers the R642 enum-variant form (`checked = OnVariant`
/// matches `state.0` against an enum carried by the first tuple elem
/// or by the state itself when non-tuple). `BoolField` is the new
/// tuple-second-elem direct bool extraction
/// (`checked = bool_field(1)`) for `(EnumState, bool)` widgets where
/// the on/off semantic isn't an enum variant — Cat A retrofit
/// (R652) target shape.
enum StateFlagSource {
    Variant(Ident),
    BoolField(usize),
}

#[derive(Default)]
struct StateFlagsConfig {
    hovered: Option<Ident>,
    pressed: Option<Ident>,
    disabled: Option<Ident>,
    /// (R653 §5.16) Accepts either an enum-variant ident
    /// (`checked = OnVariant`) or the tuple-second-elem direct bool
    /// extraction (`checked = bool_field(N)`). The other three flags
    /// (`hovered` / `pressed` / `disabled`) are always interaction-
    /// state-enum dispatched, so they keep the simpler `Ident` slot.
    checked: Option<StateFlagSource>,
    /// (R696 §5.16) WAI-ARIA `aria-expanded` for disclosure controls.
    /// Same two accepted forms as `checked` (enum-variant or
    /// `bool_field(N)`); the canonical disclosure shape is
    /// `(DisclosureState, bool)` so the usual spelling is
    /// `expanded = bool_field(1)`. Maps to
    /// `AccessNode::expanded`,
    /// a distinct axis from `checked` (see that field's docs).
    expanded: Option<StateFlagSource>,
}

struct WidgetArgs {
    tag: TagSource,
    state: Type,
    event: Type,
    title: LitStr,
    renderer: Type,
    initial_size: (Expr, Expr),
    external: Expr,
    /// (R1249 §5.45) Optional `extra_externals = <factory>` — `None`
    /// leaves the `WidgetCore::create_extra_externals` trait default
    /// (empty) in place; `Some(expr)` emits an override returning
    /// `expr()`.
    extra_externals: Option<Expr>,
    role: Option<Ident>,
    /// R1570.2 §5.16 — `apply_key = <strategy>`, the DERIVED alternative to
    /// the bare `apply_key` flag's forward-to-inherent. `None` means the
    /// binding either forwards (bare flag) or declares no `apply_key` at all.
    apply_key_strategy: Option<Ident>,
    state_flags: StateFlagsConfig,
    /// (R653 §5.16) Optional `access_value = bool_field(N)` top-level
    /// arg that adds a `.with_value(AccessValue::Bool(state.N))` chain
    /// to the derived `WidgetA11y::access_node` body. Mandatory for
    /// value-bearing ARIA roles (`Switch` / `CheckBox` / `RadioButton`
    /// — Cat A retrofit targets per R652). Only the `Bool` `AccessValue`
    /// variant is supported today; `Float` / `Int` await their 2nd
    /// consumer per [[abstraction-needs-second-consumer]]
    /// (e.g. `Slider` Cat B retrofit when value-sidecar substrate
    /// research lands).
    access_value: Option<StateFlagSource>,
    flags: HashSet<String>,
}

/// Mutable accumulator the [`WidgetArgs::parse`] loop fills from the
/// `WidgetArg` stream; finalisation enforces the cross-field
/// invariants and the required-attribute set.
#[derive(Default)]
struct WidgetArgsBuilder {
    tag: Option<TagSource>,
    state: Option<Type>,
    event: Option<Type>,
    title: Option<LitStr>,
    renderer: Option<Type>,
    initial_size: Option<(Expr, Expr)>,
    external: Option<Expr>,
    extra_externals: Option<Expr>,
    role: Option<Ident>,
    apply_key_strategy: Option<Ident>,
    state_flags: Option<StateFlagsConfig>,
    access_value: Option<StateFlagSource>,
    flags: HashSet<String>,
}

impl WidgetArgsBuilder {
    fn absorb(&mut self, item: WidgetArg) -> syn::Result<()> {
        match item {
            WidgetArg::Tag(v) => assign(&mut self.tag, v, "tag", tag_source_span),
            WidgetArg::State(v) => assign(&mut self.state, v, "state", Spanned::span),
            WidgetArg::Event(v) => assign(&mut self.event, v, "event", Spanned::span),
            WidgetArg::Title(v) => assign(&mut self.title, v, "title", Spanned::span),
            WidgetArg::Renderer(v) => assign(&mut self.renderer, v, "renderer", Spanned::span),
            WidgetArg::InitialSize(w, h) => {
                if self.initial_size.is_some() {
                    return Err(syn::Error::new(
                        w.span(),
                        "duplicate 'initial_size' attribute",
                    ));
                }
                self.initial_size = Some((w, h));
                Ok(())
            }
            WidgetArg::External(v) => assign(&mut self.external, v, "external", Spanned::span),
            WidgetArg::ExtraExternals(v) => assign(
                &mut self.extra_externals,
                v,
                "extra_externals",
                Spanned::span,
            ),
            WidgetArg::Role(v) => assign(&mut self.role, v, "role", |i: &Ident| i.span()),
            WidgetArg::ApplyKeyStrategy(v) => {
                assign(&mut self.apply_key_strategy, v, "apply_key", |i: &Ident| {
                    i.span()
                })
            }
            WidgetArg::StateFlags(cfg, span) => {
                if self.state_flags.is_some() {
                    return Err(syn::Error::new(span, "duplicate 'state_flags' attribute"));
                }
                self.state_flags = Some(cfg);
                Ok(())
            }
            WidgetArg::Flag(name, span) => {
                if !self.flags.insert(name.clone()) {
                    return Err(syn::Error::new(span, format!("duplicate '{name}' flag")));
                }
                Ok(())
            }
            WidgetArg::AccessValue(source, span) => {
                if self.access_value.is_some() {
                    return Err(syn::Error::new(span, "duplicate 'access_value' attribute"));
                }
                self.access_value = Some(source);
                Ok(())
            }
        }
    }

    fn finish(self, input_span: proc_macro2::Span) -> syn::Result<WidgetArgs> {
        // R642 §5.16 — `state_flags(...)` only makes sense alongside
        // `role = X` because the derived `access_node` body anchors on
        // both. Bare `state_flags` with no `role` would silently drop
        // the mapping (the inherent-path branch ignores it); we reject
        // at parse time so the author catches the mistake.
        // R1570.2 §5.16 — the two `apply_key` forms are alternatives: one
        // forwards to an inherent fn, the other derives a body. Accepting both
        // would make the emitted method depend on which the reader noticed.
        if self.apply_key_strategy.is_some() && self.flags.contains("apply_key") {
            return Err(syn::Error::new(
                input_span,
                "'apply_key' is declared twice: as a bare flag (forward to an \
                 inherent fn) and as 'apply_key = <strategy>' (derive the \
                 body). Keep one",
            ));
        }
        if let Some(cfg) = &self.state_flags {
            if self.role.is_none() && cfg.has_any() {
                return Err(syn::Error::new(
                    input_span,
                    "'state_flags(...)' requires 'role = <AriaRole>' (the derived \
                     access_node body anchors on both)",
                ));
            }
        }
        // R653 §5.16 — same anchor rule for `access_value`. Without a
        // derived `access_node` body (`role = X`) there is nowhere to
        // chain `.with_value(...)` onto, so a bare `access_value` is a
        // typo we'd rather catch at parse time.
        if self.access_value.is_some() && self.role.is_none() {
            return Err(syn::Error::new(
                input_span,
                "'access_value = ...' requires 'role = <AriaRole>' (the chain \
                 attaches to the derived access_node body)",
            ));
        }
        // R1249 §5.45 — hosting `extra_externals` reshapes the state scene
        // from `Scene::External(primary)` to `Scene::Container([primary,
        // ...extras])`. The DERIVED `read_state` (`read_state_derive` /
        // `state_name_derive`) pattern-matches `Scene::External` directly,
        // so it would fall through to the default state on a Container and
        // silently read nothing (the R832 multi-External no-op class).
        // Reject the combination at parse time: an extras-hosting binding
        // must supply an inherent `read_state` that walks the container via
        // `Scene::find_external_with_tag(Self::tag())`.
        if self.extra_externals.is_some()
            && (self.flags.contains("read_state_derive")
                || self.flags.contains("state_name_derive"))
        {
            return Err(syn::Error::new(
                input_span,
                "'extra_externals = ...' is incompatible with 'read_state_derive' / \
                 'state_name_derive': the derived read_state matches `Scene::External` \
                 directly and cannot walk the `Scene::Container` shape extras impose. \
                 Provide an inherent `read_state` that calls \
                 `Scene::find_external_with_tag(Self::tag())` instead.",
            ));
        }
        let missing = |field: &str| {
            syn::Error::new(
                input_span,
                format!("missing required 'widget' attribute: {field}"),
            )
        };
        Ok(WidgetArgs {
            tag: self.tag.ok_or_else(|| missing("tag"))?,
            state: self.state.ok_or_else(|| missing("state"))?,
            event: self.event.ok_or_else(|| missing("event"))?,
            title: self.title.ok_or_else(|| missing("title"))?,
            renderer: self.renderer.ok_or_else(|| missing("renderer"))?,
            initial_size: self.initial_size.ok_or_else(|| missing("initial_size"))?,
            external: self.external.ok_or_else(|| missing("external"))?,
            extra_externals: self.extra_externals,
            role: self.role,
            apply_key_strategy: self.apply_key_strategy,
            state_flags: self.state_flags.unwrap_or_default(),
            access_value: self.access_value,
            flags: self.flags,
        })
    }
}

fn tag_source_span(t: &TagSource) -> proc_macro2::Span {
    match t {
        TagSource::Lit(lit) => lit.span(),
        TagSource::Path(path) => path.span(),
    }
}

/// Slot writer that rejects duplicates with a span pointing at the
/// duplicated value. Shared by every singleton [`WidgetArg`] variant
/// (the multi-value `Flag` + `InitialSize` cases stay inline).
fn assign<T, F>(slot: &mut Option<T>, value: T, name: &str, span_of: F) -> syn::Result<()>
where
    F: FnOnce(&T) -> proc_macro2::Span,
{
    if slot.is_some() {
        return Err(syn::Error::new(
            span_of(&value),
            format!("duplicate '{name}' attribute"),
        ));
    }
    *slot = Some(value);
    Ok(())
}

impl Parse for WidgetArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut builder = WidgetArgsBuilder::default();
        let items: Punctuated<WidgetArg, Token![,]> = Punctuated::parse_terminated(input)?;
        for item in items {
            builder.absorb(item)?;
        }
        builder.finish(input.span())
    }
}

impl StateFlagsConfig {
    fn has_any(&self) -> bool {
        self.hovered.is_some()
            || self.pressed.is_some()
            || self.disabled.is_some()
            || self.checked.is_some()
            || self.expanded.is_some()
    }
}

enum WidgetArg {
    Tag(TagSource),
    State(Type),
    Event(Type),
    Title(LitStr),
    Renderer(Type),
    InitialSize(Expr, Expr),
    External(Expr),
    /// (R1249 §5.16 §5.45) `extra_externals = <factory>` — an optional
    /// nullary factory whose `Vec<ExtraExternal>` the emitted
    /// `WidgetCore::create_extra_externals` returns, so a macro binding
    /// can host sibling `External`s (an inline `TextField`, a scrollbar)
    /// without dropping to a hand-written `impl WidgetCore`. Absent → the
    /// trait default (empty) stands; the 65 pre-R1249 extra-hosting
    /// bindings that hand-rolled this method proved the gap
    /// ([[abstraction-needs-second-consumer]]). `hello-inspector` is the
    /// first consumer.
    ExtraExternals(Expr),
    Role(Ident),
    /// R1570.2 §5.16 — `apply_key = <strategy>`. The bare `apply_key` flag
    /// forwards to an inherent fn; this form names a body the macro DERIVES,
    /// which is what 25 bindings were hand-writing character for character.
    ApplyKeyStrategy(Ident),
    StateFlags(StateFlagsConfig, proc_macro2::Span),
    /// (R653 §5.16) `access_value = bool_field(N)` parsed into the
    /// same `StateFlagSource` enum the `state_flags(checked = ...)`
    /// slot uses; today only the `BoolField` variant is meaningful
    /// here (a bare ident `Variant` would emit `AccessValue::Bool(
    /// matches!(state.0, EnumType::Variant))` which is not a
    /// well-typed `AccessValue` and would silently mis-typecheck
    /// against `AccessValue::Bool(bool)` — the finaliser rejects
    /// the Variant form via `expect_bool_field`).
    AccessValue(StateFlagSource, proc_macro2::Span),
    Flag(String, proc_macro2::Span),
}

/// R1570.2 §5.16 — the bodies `apply_key = <strategy>` can DERIVE.
///
/// One entry today, and it is the one 25 bindings were writing out character
/// for character. A second is already implied by the tree — `menu_apply_key`,
/// the command-menu family's canonical body (R772.1) — which is why this is a
/// named strategy rather than a second bare flag: `apply_key_aria` would have
/// to be joined by `apply_key_menu`, and the pair would say nothing about
/// being alternatives.
const APPLY_KEY_STRATEGIES: &[&str] = &["aria_activate"];

const KNOWN_FLAGS: &[&str] = &[
    "apply_key",
    "keybinding",
    "state_name_derive",     // R643 combined; kept as alias for R642 bindings
    "read_state_derive",     // R645 split
    "event_name_derive",     // R645 split
    "fmt_state_log",         // R645 restore (2nd consumer: Slider)
    "a11y_manual",           // R645: opt out of WidgetA11y emit (binding provides custom impl)
    "update",                // R653: forward WidgetCore::update (R27 reducer side effects)
    "initial_size_strategy", // R670: forward WidgetView::initial_size_strategy to inherent fn (IntrinsicAfterFirstPaint first consumer)
];

impl Parse for WidgetArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        let key_str = key.to_string();
        if input.peek(Token![=]) {
            let _: Token![=] = input.parse()?;
            match key_str.as_str() {
                "tag" => {
                    // R644 §5.16 — accept both `tag = "main_btn"`
                    // (R641 literal form) and `tag = Tags::MainBtn`
                    // (R644 typed form). Peek for `LitStr` first
                    // so a bare ident path doesn't get reparsed as
                    // a string-keyed expression.
                    if input.peek(LitStr) {
                        Ok(Self::Tag(TagSource::Lit(input.parse()?)))
                    } else {
                        Ok(Self::Tag(TagSource::Path(input.parse()?)))
                    }
                }
                "state" => Ok(Self::State(input.parse()?)),
                "event" => Ok(Self::Event(input.parse()?)),
                "title" => Ok(Self::Title(input.parse()?)),
                "renderer" => Ok(Self::Renderer(input.parse()?)),
                "initial_size" => {
                    let content;
                    syn::parenthesized!(content in input);
                    let w: Expr = content.parse()?;
                    let _: Token![,] = content.parse()?;
                    let h: Expr = content.parse()?;
                    Ok(Self::InitialSize(w, h))
                }
                "external" => Ok(Self::External(input.parse()?)),
                "extra_externals" => Ok(Self::ExtraExternals(input.parse()?)),
                "role" => Ok(Self::Role(input.parse()?)),
                "apply_key" => {
                    let strategy: Ident = input.parse()?;
                    if !APPLY_KEY_STRATEGIES.contains(&strategy.to_string().as_str()) {
                        return Err(syn::Error::new(
                            strategy.span(),
                            format!(
                                "unknown 'apply_key' strategy: '{strategy}' — \
                                 accepted: {}. Omit the '= <strategy>' to \
                                 forward to an inherent fn instead",
                                APPLY_KEY_STRATEGIES.join(" / "),
                            ),
                        ));
                    }
                    Ok(Self::ApplyKeyStrategy(strategy))
                }
                "access_value" => {
                    // R653 §5.16 — accept only the `bool_field(N)`
                    // call form. The bare-ident form is reserved for
                    // enum-variant dispatch in `state_flags`; for the
                    // AccessValue chain the macro can only emit
                    // `AccessValue::Bool(state.N)` today.
                    let head: Ident = input.parse()?;
                    if !input.peek(syn::token::Paren) || head != "bool_field" {
                        return Err(syn::Error::new(
                            head.span(),
                            "'access_value = bool_field(N)' is the only \
                             accepted form today (AccessValue::Bool variant; \
                             Float/Int await their 2nd consumer)",
                        ));
                    }
                    let content;
                    syn::parenthesized!(content in input);
                    let idx_lit: syn::LitInt = content.parse()?;
                    let idx: usize = idx_lit.base10_parse()?;
                    Ok(Self::AccessValue(
                        StateFlagSource::BoolField(idx),
                        key.span(),
                    ))
                }
                _ => Err(syn::Error::new(
                    key.span(),
                    format!("unknown widget attribute: '{key_str}'"),
                )),
            }
        } else if input.peek(syn::token::Paren) {
            // `name(...)` form — currently `state_flags(...)`.
            match key_str.as_str() {
                "state_flags" => {
                    let content;
                    syn::parenthesized!(content in input);
                    let cfg = parse_state_flags(&content)?;
                    Ok(Self::StateFlags(cfg, key.span()))
                }
                _ => Err(syn::Error::new(
                    key.span(),
                    format!(
                        "unknown widget attribute group: '{key_str}(...)' — \
                         only 'state_flags(...)' accepted at this position",
                    ),
                )),
            }
        } else {
            // Bare-identifier flag form.
            if KNOWN_FLAGS.contains(&key_str.as_str()) {
                Ok(Self::Flag(key_str, key.span()))
            } else {
                Err(syn::Error::new(
                    key.span(),
                    format!(
                        "unknown widget flag '{key_str}' — expected one of: {}",
                        KNOWN_FLAGS.join(", "),
                    ),
                ))
            }
        }
    }
}

/// Parse the inside of `state_flags(...)`. Accepts a comma-separated
/// list of `flag = <source>` pairs where `flag` is one of `hovered` /
/// `pressed` / `disabled` / `checked`. The `<source>` form depends on
/// the flag: `hovered` / `pressed` / `disabled` accept only a bare
/// variant ident (`Hover`, `Pressed`, `Disabled`) since interaction
/// state is always enum-variant dispatched; `checked` accepts either
/// a bare variant ident OR the R653 §5.16 `bool_field(N)` call form
/// for tuple-state second-elem direct bool extraction.
fn parse_state_flags(input: ParseStream) -> syn::Result<StateFlagsConfig> {
    let mut cfg = StateFlagsConfig::default();
    let entries: Punctuated<StateFlagEntry, Token![,]> = Punctuated::parse_terminated(input)?;
    for entry in entries {
        let StateFlagEntry { name, source } = entry;
        match name.to_string().as_str() {
            "hovered" => {
                let variant = require_variant(&name, "hovered", source)?;
                if cfg.hovered.is_some() {
                    return Err(syn::Error::new(
                        name.span(),
                        "duplicate 'hovered' state_flag",
                    ));
                }
                cfg.hovered = Some(variant);
            }
            "pressed" => {
                let variant = require_variant(&name, "pressed", source)?;
                if cfg.pressed.is_some() {
                    return Err(syn::Error::new(
                        name.span(),
                        "duplicate 'pressed' state_flag",
                    ));
                }
                cfg.pressed = Some(variant);
            }
            "disabled" => {
                let variant = require_variant(&name, "disabled", source)?;
                if cfg.disabled.is_some() {
                    return Err(syn::Error::new(
                        name.span(),
                        "duplicate 'disabled' state_flag",
                    ));
                }
                cfg.disabled = Some(variant);
            }
            "checked" => {
                if cfg.checked.is_some() {
                    return Err(syn::Error::new(
                        name.span(),
                        "duplicate 'checked' state_flag",
                    ));
                }
                cfg.checked = Some(source);
            }
            // R696 §5.16 — `aria-expanded` for disclosure controls.
            // Same source forms as `checked` (enum-variant or
            // `bool_field(N)`); maps to `AccessNode::expanded`.
            "expanded" => {
                if cfg.expanded.is_some() {
                    return Err(syn::Error::new(
                        name.span(),
                        "duplicate 'expanded' state_flag",
                    ));
                }
                cfg.expanded = Some(source);
            }
            other => {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "unknown state_flag '{other}' — expected one of: \
                         hovered, pressed, disabled, checked, expanded",
                    ),
                ));
            }
        }
    }
    Ok(cfg)
}

struct StateFlagEntry {
    name: Ident,
    source: StateFlagSource,
}

impl Parse for StateFlagEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let _: Token![=] = input.parse()?;
        let head: Ident = input.parse()?;
        // R653 §5.16 — call form `bool_field(N)` distinguishes the
        // tuple-second-elem direct bool extraction from the bare
        // enum-variant form. Only `bool_field` is currently accepted
        // as a callable; any other call form is a parse-time reject so
        // a typo cannot silently land as an unknown ident.
        let source = if input.peek(syn::token::Paren) {
            if head != "bool_field" {
                return Err(syn::Error::new(
                    head.span(),
                    format!(
                        "unknown state_flag source form '{head}(...)' — \
                         only 'bool_field(N)' is accepted as a callable; \
                         use a bare variant ident for the enum form",
                    ),
                ));
            }
            let content;
            syn::parenthesized!(content in input);
            let idx_lit: syn::LitInt = content.parse()?;
            let idx: usize = idx_lit.base10_parse()?;
            StateFlagSource::BoolField(idx)
        } else {
            StateFlagSource::Variant(head)
        };
        Ok(Self { name, source })
    }
}

/// (R653 §5.16) Validate that a `state_flags(...)` entry uses the
/// enum-variant form rather than `bool_field(N)`. `hovered` / `pressed`
/// / `disabled` are always interaction-state-enum dispatched (a `bool`
/// field cannot encode the "currently hovered" semantic), so the
/// parser rejects the call form for those three slots with a span
/// pointed at the flag name.
fn require_variant(name: &Ident, flag: &str, source: StateFlagSource) -> syn::Result<Ident> {
    match source {
        StateFlagSource::Variant(v) => Ok(v),
        StateFlagSource::BoolField(_) => Err(syn::Error::new(
            name.span(),
            format!(
                "'{flag}' state_flag requires a bare enum-variant ident; \
                 the 'bool_field(N)' call form is accepted only for 'checked' \
                 / 'expanded' (interaction state is always enum-variant \
                 dispatched)",
            ),
        )),
    }
}

#[cfg(test)]
mod forward_guard_tests {
    //! R1570 §5.16 — every forward this macro emits carries its guard.
    //!
    //! This is a **census**, not a spot check. A body reading
    //! `<TheView>::name(..)` inside the trait method named `name` calls itself
    //! whenever the view declares no inherent function, so a forward added
    //! without a guard reintroduces the exact defect R1570 closed — and
    //! reintroduces it *silently*, since the tree only notices when some
    //! binding forgets that particular method. So the assertion is over the
    //! whole expansion: pair up the unqualified `<View>::name` calls with the
    //! `widget_forward` imports and require the two sets to agree.
    //!
    //! Qualified calls (`<View as SomeTrait>::name(..)`) are excluded by
    //! construction, and correctly: naming the trait is itself an
    //! unambiguous resolution, so those cannot fall back to anything.

    use super::expand;
    use quote::quote;

    /// Every optional forward switched ON, no `role` (so the a11y half
    /// forwards too) and neither derive flag (so `read_state` / `event_name`
    /// forward rather than deriving) — the expansion that reaches every
    /// guarded site at once.
    fn every_forward() -> String {
        expand(
            quote! {
                tag = "probe",
                state = ProbeState,
                event = ProbeEvent,
                title = "probe",
                renderer = ProbeRenderer,
                initial_size = (10, 10),
                external = ProbeExternal::new,
                initial_size_strategy,
                apply_key,
                keybinding,
                fmt_state_log,
                update,
            },
            quote! { struct ProbeView; },
        )
        .expect("the probe attribute is well-formed")
        .to_string()
    }

    /// The identifier following each `< ProbeView > :: ` in the rendered
    /// token stream — i.e. every call that could resolve to the trait method
    /// it is standing inside.
    fn unqualified_forwards(src: &str) -> Vec<String> {
        idents_after(src, "< ProbeView > :: ")
    }

    /// The guard trait named by each `widget_forward :: ` import.
    fn imported_guards(src: &str) -> Vec<String> {
        idents_after(src, "widget_forward :: ")
    }

    fn idents_after(src: &str, marker: &str) -> Vec<String> {
        let mut out: Vec<String> = src
            .match_indices(marker)
            .map(|(at, _)| {
                src[at + marker.len()..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect()
            })
            .collect();
        out.sort();
        out
    }

    /// `read_state` -> `ReadStateNeedsInherentFn`, the naming the guard module
    /// and this macro have to agree on. Derived rather than tabulated, so a
    /// new forward cannot be added to one side and typoed into the other.
    fn guard_for(method: &str) -> String {
        let mut name = String::new();
        let mut upper = true;
        for c in method.chars() {
            if c == '_' {
                upper = true;
            } else if upper {
                name.extend(c.to_uppercase());
                upper = false;
            } else {
                name.push(c);
            }
        }
        name.push_str("NeedsInherentFn");
        name
    }

    #[test]
    fn every_unqualified_forward_is_guarded() {
        let src = every_forward();
        let forwards = unqualified_forwards(&src);
        assert!(
            !forwards.is_empty(),
            "the probe expansion emitted no forwards at all — the fixture \
             stopped exercising what it audits"
        );

        let mut expected: Vec<String> = forwards.iter().map(|m| guard_for(m)).collect();
        expected.sort();
        assert_eq!(
            imported_guards(&src),
            expected,
            "a `<View>::name(..)` forward is emitted without \
             `pinion_core::widget_forward`'s guard for that name (or with the \
             wrong one). Such a forward calls itself when the binding declares \
             no inherent `name` — see R1570."
        );
    }

    #[test]
    fn the_probe_reaches_all_nine_forwards() {
        // Named individually so that *losing* a forwarding site — which would
        // make the census above pass by covering less — fails here instead.
        let forwards = unqualified_forwards(&every_forward());
        for method in [
            "access_node",
            "apply_key",
            "event_name",
            "fmt_state_log",
            "initial_size_strategy",
            "keybinding",
            "read_state",
            "update",
            "view",
        ] {
            assert!(
                forwards.iter().any(|f| f == method),
                "the probe expansion no longer contains the `{method}` forward"
            );
        }
        assert_eq!(forwards.len(), 9, "forwards = {forwards:?}");
    }

    #[test]
    fn a_derived_half_emits_no_forward_and_so_needs_no_guard() {
        // The negative control for the census: when `read_state` / `event_name`
        // are DERIVED there is no self-call to guard, and the guard must not be
        // imported for them either — an import with no forward would make the
        // set comparison pass for the wrong reason.
        let src = expand(
            quote! {
                tag = "probe",
                state = ProbeState,
                event = ProbeEvent,
                title = "probe",
                renderer = ProbeRenderer,
                initial_size = (10, 10),
                external = ProbeExternal::new,
                state_name_derive,
            },
            quote! { struct ProbeView; },
        )
        .expect("the probe attribute is well-formed")
        .to_string();

        let forwards = unqualified_forwards(&src);
        assert!(!forwards.iter().any(|f| f == "read_state"), "{forwards:?}");
        assert!(!forwards.iter().any(|f| f == "event_name"), "{forwards:?}");
        assert!(
            !imported_guards(&src)
                .iter()
                .any(|g| g == "ReadStateNeedsInherentFn" || g == "EventNameNeedsInherentFn"),
            "a guard was imported for a name that is derived, not forwarded"
        );
    }

    #[test]
    fn a_qualified_call_is_not_counted_as_a_forward() {
        // `<ProbeView as WidgetCore>::tag()` appears in the derived a11y body.
        // It names its trait, so it cannot resolve to anything else and needs
        // no guard — and the census must not demand one, or the two sets could
        // only be made to agree by guarding calls that are already safe.
        let src = expand(
            quote! {
                tag = "probe",
                state = ProbeState,
                event = ProbeEvent,
                title = "probe",
                renderer = ProbeRenderer,
                initial_size = (10, 10),
                external = ProbeExternal::new,
                role = Button,
                state_name_derive,
            },
            quote! { struct ProbeView; },
        )
        .expect("the probe attribute is well-formed")
        .to_string();

        assert!(
            src.contains("< ProbeView as :: pinion_core :: WidgetCore > :: tag"),
            "the fixture stopped containing a qualified call, so it no longer \
             discriminates"
        );
        assert!(!unqualified_forwards(&src).iter().any(|f| f == "tag"));
    }

    #[test]
    fn the_guard_name_derivation_matches_the_module() {
        // The two sides are written in different crates, so the mapping is
        // pinned here rather than left to reading.
        assert_eq!(guard_for("view"), "ViewNeedsInherentFn");
        assert_eq!(guard_for("read_state"), "ReadStateNeedsInherentFn");
        assert_eq!(
            guard_for("initial_size_strategy"),
            "InitialSizeStrategyNeedsInherentFn"
        );
    }
}

#[cfg(test)]
mod apply_key_strategy_tests {
    //! R1570.2 §5.16 — the derived `apply_key` is a body, not a forward.
    //!
    //! The two forms are alternatives and the difference is structural: the
    //! bare flag emits a call the binding must have an inherent fn for (and so
    //! carries R1570's guard), while `= <strategy>` emits the body itself (and
    //! so must NOT carry one — a guard there would demand an inherent fn the
    //! whole point is not to need).

    use super::expand;
    use quote::quote;

    fn expand_with(extra: &proc_macro2::TokenStream) -> syn::Result<String> {
        expand(
            quote! {
                tag = "probe",
                state = ProbeState,
                event = ProbeEvent,
                title = "probe",
                renderer = ProbeRenderer,
                initial_size = (10, 10),
                external = ProbeExternal::new,
                #extra
            },
            quote! { struct ProbeView; },
        )
        .map(|ts| ts.to_string())
    }

    #[test]
    fn the_strategy_emits_the_body_and_no_guard() {
        let src = expand_with(&quote! { apply_key = aria_activate, }).expect("well-formed");
        assert!(
            src.contains("apply_aria_activate"),
            "the derived body calls the substrate"
        );
        assert!(
            src.contains("< ProbeView as :: pinion_core :: WidgetCore > :: tag"),
            "the tag is reached through a QUALIFIED path, so the emitted body \
             does not depend on which trait `Self` resolves through"
        );
        assert!(
            !src.contains("ApplyKeyNeedsInherentFn"),
            "a derived body needs no inherent fn, so guarding it would demand \
             the very thing the strategy exists to avoid"
        );
        assert!(
            !src.contains("< ProbeView > :: apply_key"),
            "and it must not ALSO forward — that would be the recursion R1570 \
             closed, reintroduced through the other door"
        );
    }

    #[test]
    fn the_bare_flag_still_forwards_under_its_guard() {
        // The negative control for the test above: without it, deleting the
        // forwarding arm entirely would satisfy every assertion there.
        let src = expand_with(&quote! { apply_key, }).expect("well-formed");
        assert!(src.contains("< ProbeView > :: apply_key"));
        assert!(src.contains("ApplyKeyNeedsInherentFn"));
        assert!(!src.contains("apply_aria_activate"));
    }

    #[test]
    fn declaring_both_forms_is_refused() {
        // They are alternatives; accepting both would make the emitted method
        // depend on which one the reader happened to notice.
        let err = expand_with(&quote! { apply_key, apply_key = aria_activate, })
            .expect_err("both forms must be refused");
        assert!(err.to_string().contains("declared twice"), "{err}");
    }

    #[test]
    fn an_unknown_strategy_names_the_accepted_set() {
        let err = expand_with(&quote! { apply_key = teleport, })
            .expect_err("an unknown strategy must be refused");
        let msg = err.to_string();
        assert!(msg.contains("teleport"), "{msg}");
        assert!(
            msg.contains("aria_activate"),
            "the refusal names what IS accepted: {msg}"
        );
    }

    #[test]
    fn no_apply_key_at_all_emits_neither() {
        let src = expand_with(&quote! {}).expect("well-formed");
        assert!(!src.contains("fn apply_key"));
        assert!(!src.contains("ApplyKeyNeedsInherentFn"));
    }
}
