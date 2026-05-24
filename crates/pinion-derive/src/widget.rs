//! R641 §5.16 — `#[pinion::widget]` attribute macro.
//!
//! Lifts the mechanical wiring of the [`WidgetCore`] / [`WidgetA11y`] /
//! [`WidgetView`] supertrait chain into a single attribute attached to
//! the widget unit struct, leaving the widget-specific logic
//! (`read_state` / `event_name` / `view`) as inherent methods the macro
//! forwards into. R642 §5.16 added the declarative `role` +
//! `state_flags(...)` attributes that auto-derive the single-node
//! [`WidgetA11y::access_node`] body for the 80 % case (`Button`-shaped
//! widgets — see [`crate::widget`] module docs for the variant table),
//! leaving the inherent fn path open as the escape hatch for
//! composite widgets (`RadioGroup`, `Listbox`).
//!
//! ## Why an attribute macro, not three derives
//!
//! The three traits are intentionally split across crates so a TUI
//! binding can replace [`WidgetView`] (Vello-bound) with
//! [`WidgetViewTui`] (ratatui-bound) without losing the shared
//! [`WidgetCore`] + [`WidgetA11y`] surface. Three separate `#[derive(...)]`
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
//! - `tag = "main_btn"` — paint-side dispatch tag the [`InputRouter`]
//!   hit-tests against. Returned from [`WidgetCore::tag`].
//! - `state = ButtonState` — [`WidgetCore::State`] associated type.
//! - `event = ButtonEvent` — [`WidgetCore::Event`] associated type.
//! - `title = "Hello Button"` — OS window title. Returned from
//!   [`WidgetCore::title`].
//! - `renderer = HelloButtonRenderer` — [`WidgetView::Renderer`]
//!   associated type (pinion-forge-emitted Vello renderer struct).
//! - `initial_size = (W, H)` — logical-pixel default window size.
//!   Returned from [`WidgetView::initial_size`].
//! - `external = ButtonExternal::new` — factory expression invoked
//!   inside [`WidgetCore::create_external`] (`Box::new(<expr>())`).
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
//!   single-node [`WidgetA11y::access_node`] body that matches the
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
//! `role` selects the [`AriaRole`] variant the derived node carries
//! (`Button`, `Switch`, `CheckBox`, `Slider`, `TextInput`,
//! `RadioButton` — single-node roles).
//!
//! `state_flags(...)` declares the state-variant → [`AccessState`]
//! bool-flag mapping. Each entry is `flag = Variant` where `flag` is
//! one of `hovered` / `pressed` / `disabled` / `checked` and
//! `Variant` is a bare unit-variant ident of `Self::State`:
//!
//! | Flag       | Maps to                              | Variant absent → |
//! | ---------- | ------------------------------------ | ---------------- |
//! | `hovered`  | `matches!(state, State::Variant)`    | `false`          |
//! | `pressed`  | `matches!(state, State::Variant)`    | `false`          |
//! | `disabled` | `matches!(state, State::Variant)`    | `false`          |
//! | `checked`  | `Some(matches!(state, State::Variant))` | `None`        |
//!
//! `focused` is auto-derived from `focused == Some(<Self as WidgetCore>::tag())`
//! for every declarative node — no `state_flags` entry needed.
//!
//! Multi-field state structs (`CheckboxState { ..., checked: bool }`)
//! aren't reachable from `state_flags` v0 — those bindings stay on the
//! inherent path until a future macro round teaches the parser the
//! field-access form (`checked = field(checked)`). R642 covers the
//! enum-variant shape (`ButtonState::Hover`, `ToggleState::On`) the
//! survey identified as the 80 % case.
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
//! | Flag         | Trait default (no flag)           | When to opt in                                       |
//! | ------------ | --------------------------------- | ---------------------------------------------------- |
//! | `apply_key`  | `false` (no key handled)          | ARIA Space/Enter activation, Arrow keys, custom keys |
//! | `keybinding` | `None` (no character-key mapping) | Single-char shortcuts mapping to `Self::Event`       |
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
//!
//! [`InputRouter`]: pinion_core::scene
//! [`WidgetCore`]: pinion_core::WidgetCore
//! [`WidgetA11y`]: pinion_a11y::WidgetA11y
//! [`WidgetA11y::access_node`]: pinion_a11y::WidgetA11y::access_node
//! [`WidgetView`]: pinion_shell::WidgetView
//! [`WidgetViewTui`]: pinion_tui::WidgetViewTui
//! [`AriaRole`]: pinion_a11y::AriaRole
//! [`AccessState`]: pinion_a11y::AccessState

use std::collections::HashSet;

use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    spanned::Spanned,
    Expr, Ident, ItemStruct, LitStr, Token, Type,
};

/// Entry point for [`crate::widget`]. Parses the attribute and the
/// item, then assembles the three forwarding trait impls.
pub(crate) fn expand(
    attr: TokenStream2,
    item: TokenStream2,
) -> syn::Result<TokenStream2> {
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
        role,
        state_flags,
        flags,
    } = args;

    let optional_forwards = emit_optional_forwards(view_ident, &event, &flags);
    let a11y_impl = emit_a11y_impl(view_ident, &state, role.as_ref(), &state_flags);

    Ok(quote! {
        #item

        impl ::pinion_core::WidgetCore for #view_ident {
            type State = #state;
            type Event = #event;

            fn tag() -> &'static str { #tag }
            fn title() -> &'static str { #title }

            fn create_external() -> ::std::boxed::Box<dyn ::pinion_core::external::External> {
                ::std::boxed::Box::new(#external())
            }

            fn read_state(scene: &::pinion_core::Scene) -> #state {
                <#view_ident>::read_state(scene)
            }

            fn event_name(event: #event) -> &'static str {
                <#view_ident>::event_name(event)
            }

            fn view(state: #state, frame: &::pinion_core::Frame) -> ::pinion_core::Scene {
                // R641 §5.16 — bridge trait by-ref (`&Frame`) to the
                // by-value inherent fn signature the workspace
                // `clippy::pedantic` `trivially_copy_pass_by_ref` lint
                // prefers (`Frame` is `Copy` + 4 bytes today). Macro
                // is the boundary; user code stays clippy-clean.
                <#view_ident>::view(state, *frame)
            }

            #optional_forwards
        }

        #a11y_impl

        impl ::pinion_shell::WidgetView for #view_ident {
            type Renderer = #renderer;
            fn initial_size() -> (u32, u32) { (#init_w, #init_h) }
        }
    })
}

fn emit_optional_forwards(
    view_ident: &Ident,
    event: &Type,
    flags: &HashSet<String>,
) -> TokenStream2 {
    let mut out = TokenStream2::new();
    if flags.contains("apply_key") {
        out.extend(quote! {
            fn apply_key(
                scene: &mut ::pinion_core::Scene,
                focused: ::core::option::Option<&str>,
                key: &str,
                modifiers: ::pinion_core::input::Modifiers,
            ) -> bool {
                <#view_ident>::apply_key(scene, focused, key, modifiers)
            }
        });
    }
    if flags.contains("keybinding") {
        out.extend(quote! {
            fn keybinding(key: &str) -> ::core::option::Option<#event> {
                <#view_ident>::keybinding(key)
            }
        });
    }
    out
}

/// R642 §5.16 — emit the [`WidgetA11y`] impl. Two paths:
///
/// - When `role` is supplied → emit a derived single-node body using
///   the declarative `state_flags(...)` mapping (80 % case). No
///   inherent `fn access_node` required on the unit struct.
/// - When `role` is absent → forward to `<#view_ident>::access_node`,
///   preserving R641 behaviour for composite widgets that need the
///   inherent escape hatch (`RadioGroup`, `Listbox`).
///
/// [`WidgetA11y`]: pinion_a11y::WidgetA11y
fn emit_a11y_impl(
    view_ident: &Ident,
    state: &Type,
    role: Option<&Ident>,
    state_flags: &StateFlagsConfig,
) -> TokenStream2 {
    if let Some(role_ident) = role {
        let bool_flag = |v: &Option<Ident>| {
            v.as_ref().map_or_else(
                || quote! { false },
                |variant| quote! { ::core::matches!(state, #state::#variant) },
            )
        };
        let hovered_expr = bool_flag(&state_flags.hovered);
        let pressed_expr = bool_flag(&state_flags.pressed);
        let disabled_expr = bool_flag(&state_flags.disabled);
        let checked_expr = state_flags.checked.as_ref().map_or_else(
            || quote! { ::core::option::Option::None },
            |v| quote! { ::core::option::Option::Some(::core::matches!(state, #state::#v)) },
        );

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
                    };
                    ::std::vec![
                        ::pinion_a11y::AccessNode::new(
                            <#view_ident as ::pinion_core::WidgetCore>::tag(),
                            ::pinion_a11y::AriaRole::#role_ident,
                        )
                        .with_state(access_state)
                    ]
                }
            }
        }
    } else {
        quote! {
            impl ::pinion_a11y::WidgetA11y for #view_ident {
                fn access_node(
                    state: &#state,
                    focused: ::core::option::Option<&str>,
                ) -> ::std::vec::Vec<::pinion_a11y::AccessNode> {
                    // R641 §5.16 — bridge trait `&State` to the
                    // inherent by-value signature `WidgetCore::State:
                    // Copy` guarantees is always sound + matches the
                    // clippy `trivially_copy_pass_by_ref` preference
                    // for Copy ZST / one-byte enums.
                    <#view_ident>::access_node(*state, focused)
                }
            }
        }
    }
}

#[derive(Default)]
struct StateFlagsConfig {
    hovered: Option<Ident>,
    pressed: Option<Ident>,
    disabled: Option<Ident>,
    checked: Option<Ident>,
}

struct WidgetArgs {
    tag: LitStr,
    state: Type,
    event: Type,
    title: LitStr,
    renderer: Type,
    initial_size: (Expr, Expr),
    external: Expr,
    role: Option<Ident>,
    state_flags: StateFlagsConfig,
    flags: HashSet<String>,
}

impl Parse for WidgetArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut tag: Option<LitStr> = None;
        let mut state: Option<Type> = None;
        let mut event: Option<Type> = None;
        let mut title: Option<LitStr> = None;
        let mut renderer: Option<Type> = None;
        let mut initial_size: Option<(Expr, Expr)> = None;
        let mut external: Option<Expr> = None;
        let mut role: Option<Ident> = None;
        let mut state_flags: Option<StateFlagsConfig> = None;
        let mut flags: HashSet<String> = HashSet::new();

        let items: Punctuated<WidgetArg, Token![,]> = Punctuated::parse_terminated(input)?;
        for item in items {
            match item {
                WidgetArg::Tag(v) => {
                    if tag.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'tag' attribute"));
                    }
                    tag = Some(v);
                }
                WidgetArg::State(v) => {
                    if state.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'state' attribute"));
                    }
                    state = Some(v);
                }
                WidgetArg::Event(v) => {
                    if event.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'event' attribute"));
                    }
                    event = Some(v);
                }
                WidgetArg::Title(v) => {
                    if title.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'title' attribute"));
                    }
                    title = Some(v);
                }
                WidgetArg::Renderer(v) => {
                    if renderer.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'renderer' attribute"));
                    }
                    renderer = Some(v);
                }
                WidgetArg::InitialSize(w, h) => {
                    if initial_size.is_some() {
                        return Err(syn::Error::new(w.span(), "duplicate 'initial_size' attribute"));
                    }
                    initial_size = Some((w, h));
                }
                WidgetArg::External(v) => {
                    if external.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'external' attribute"));
                    }
                    external = Some(v);
                }
                WidgetArg::Role(v) => {
                    if role.is_some() {
                        return Err(syn::Error::new(v.span(), "duplicate 'role' attribute"));
                    }
                    role = Some(v);
                }
                WidgetArg::StateFlags(cfg, span) => {
                    if state_flags.is_some() {
                        return Err(syn::Error::new(span, "duplicate 'state_flags' attribute"));
                    }
                    state_flags = Some(cfg);
                }
                WidgetArg::Flag(name, span) => {
                    if !flags.insert(name.clone()) {
                        return Err(syn::Error::new(span, format!("duplicate '{name}' flag")));
                    }
                }
            }
        }

        // R642 §5.16 — `state_flags(...)` only makes sense alongside
        // `role = X` because the derived `access_node` body anchors on
        // both. Bare `state_flags` with no `role` would silently drop
        // the mapping (the inherent-path branch ignores it); we reject
        // at parse time so the author catches the mistake.
        if let Some(cfg) = &state_flags {
            if role.is_none() && cfg.has_any() {
                return Err(syn::Error::new(
                    input.span(),
                    "'state_flags(...)' requires 'role = <AriaRole>' (the derived \
                     access_node body anchors on both)",
                ));
            }
        }

        let missing = |field: &str| {
            syn::Error::new(input.span(), format!("missing required 'widget' attribute: {field}"))
        };
        Ok(Self {
            tag: tag.ok_or_else(|| missing("tag"))?,
            state: state.ok_or_else(|| missing("state"))?,
            event: event.ok_or_else(|| missing("event"))?,
            title: title.ok_or_else(|| missing("title"))?,
            renderer: renderer.ok_or_else(|| missing("renderer"))?,
            initial_size: initial_size.ok_or_else(|| missing("initial_size"))?,
            external: external.ok_or_else(|| missing("external"))?,
            role,
            state_flags: state_flags.unwrap_or_default(),
            flags,
        })
    }
}

impl StateFlagsConfig {
    fn has_any(&self) -> bool {
        self.hovered.is_some()
            || self.pressed.is_some()
            || self.disabled.is_some()
            || self.checked.is_some()
    }
}

enum WidgetArg {
    Tag(LitStr),
    State(Type),
    Event(Type),
    Title(LitStr),
    Renderer(Type),
    InitialSize(Expr, Expr),
    External(Expr),
    Role(Ident),
    StateFlags(StateFlagsConfig, proc_macro2::Span),
    Flag(String, proc_macro2::Span),
}

const KNOWN_FLAGS: &[&str] = &[
    "apply_key",
    "keybinding",
];

impl Parse for WidgetArg {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let key: Ident = input.parse()?;
        let key_str = key.to_string();
        if input.peek(Token![=]) {
            let _: Token![=] = input.parse()?;
            match key_str.as_str() {
                "tag" => Ok(Self::Tag(input.parse()?)),
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
                "role" => Ok(Self::Role(input.parse()?)),
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

/// Parse the inside of `state_flags(...)`. Accepts a
/// comma-separated list of `flag = Variant` pairs where `flag` is one
/// of `hovered` / `pressed` / `disabled` / `checked` and `Variant` is
/// a bare ident (one variant of the state enum named in `state = X`).
fn parse_state_flags(input: ParseStream) -> syn::Result<StateFlagsConfig> {
    let mut cfg = StateFlagsConfig::default();
    let entries: Punctuated<StateFlagEntry, Token![,]> = Punctuated::parse_terminated(input)?;
    for entry in entries {
        let StateFlagEntry { name, variant } = entry;
        match name.to_string().as_str() {
            "hovered" => {
                if cfg.hovered.is_some() {
                    return Err(syn::Error::new(name.span(), "duplicate 'hovered' state_flag"));
                }
                cfg.hovered = Some(variant);
            }
            "pressed" => {
                if cfg.pressed.is_some() {
                    return Err(syn::Error::new(name.span(), "duplicate 'pressed' state_flag"));
                }
                cfg.pressed = Some(variant);
            }
            "disabled" => {
                if cfg.disabled.is_some() {
                    return Err(syn::Error::new(name.span(), "duplicate 'disabled' state_flag"));
                }
                cfg.disabled = Some(variant);
            }
            "checked" => {
                if cfg.checked.is_some() {
                    return Err(syn::Error::new(name.span(), "duplicate 'checked' state_flag"));
                }
                cfg.checked = Some(variant);
            }
            other => {
                return Err(syn::Error::new(
                    name.span(),
                    format!(
                        "unknown state_flag '{other}' — expected one of: \
                         hovered, pressed, disabled, checked",
                    ),
                ));
            }
        }
    }
    Ok(cfg)
}

struct StateFlagEntry {
    name: Ident,
    variant: Ident,
}

impl Parse for StateFlagEntry {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let name: Ident = input.parse()?;
        let _: Token![=] = input.parse()?;
        let variant: Ident = input.parse()?;
        Ok(Self { name, variant })
    }
}
