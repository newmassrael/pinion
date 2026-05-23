//! R641 §5.16 — `#[pinion::widget]` attribute macro.
//!
//! Lifts the mechanical wiring of the [`WidgetCore`] / [`WidgetA11y`] /
//! [`WidgetView`] supertrait chain into a single attribute attached to
//! the widget unit struct, leaving the widget-specific logic
//! (`read_state` / `event_name` / `view` / `access_node`) as inherent
//! methods the macro forwards into.
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
//! The macro emits forwarding stubs for these four methods, which
//! every widget binding has widget-specific logic for and which have
//! no sensible default at the trait level:
//!
//! ```rust,ignore
//! impl ButtonView {
//!     fn view(state: ButtonState, frame: &Frame) -> Scene { ... }
//!     fn read_state(scene: &Scene) -> ButtonState { ... }
//!     fn event_name(event: ButtonEvent) -> &'static str { ... }
//!     fn access_node(state: &ButtonState, focused: Option<&str>)
//!         -> Vec<AccessNode> { ... }
//! }
//! ```
//!
//! The macro emits `<ButtonView>::view(state, frame)` etc. as the
//! trait body — if the inherent fn is missing the compile error
//! surfaces at the trait impl site with the standard "no function
//! named `view` found" message.
//!
//! ## Optional forward flags
//!
//! Flag-style attributes opt into forwarding additional methods to
//! inherent `fn` items the author provides; absent the flag, the
//! trait default applies:
//!
//! | Flag                 | Trait default (no flag)               | When to opt in                                       |
//! | -------------------- | ------------------------------------- | ---------------------------------------------------- |
//! | `apply_key`          | `false` (no key handled)              | ARIA Space/Enter activation, Arrow keys, custom keys |
//! | `keybinding`         | `None` (no character-key mapping)     | Single-char shortcuts mapping to `Self::Event`       |
//! | `apply_composition`  | `false` (no IME handled)              | Text-input widgets (TextField-class)                 |
//! | `apply_middle_click` | `false` (no middle-click handled)     | Text-input widgets (PRIMARY paste on Linux)          |
//! | `ime_caret_rect`     | `None` (no caret positioning hint)    | Text-input widgets                                   |
//! | `focusable_tags`     | `[Self::tag()]` (single focusable)    | Composite widgets (`RadioGroup`, `ListBox`)          |
//! | `access_focus_target` | atomic focus on `focused` tag        | Composite widgets with `aria-activedescendant`       |
//! | `access_child_invoke` | rejects (no composite dispatch)      | Composite widgets with AT-driven child activation    |
//! | `fmt_state_log`      | `Debug` format                        | Multi-axis state (`Toggle::fmt_state_log` precedent) |
//! | `update`             | no-op (returns empty intents)         | Widgets that consume intents in a reducer            |
//! | `create_extra_externals` | `vec![]` (single External)        | Multi-External widgets (`ListBox` + `ScrollBar`)     |
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
//!     apply_key,
//! )]
//! struct ButtonView;
//!
//! impl ButtonView {
//!     fn view(state: ButtonState, frame: &Frame) -> Scene { /* ... */ }
//!     fn read_state(scene: &Scene) -> ButtonState { /* ... */ }
//!     fn event_name(event: ButtonEvent) -> &'static str { /* ... */ }
//!     fn access_node(state: &ButtonState, focused: Option<&str>)
//!         -> Vec<AccessNode> { /* ... */ }
//!     fn apply_key(scene: &mut Scene, focused: Option<&str>, key: &str,
//!         modifiers: Modifiers) -> bool { /* ... */ }
//! }
//! ```
//!
//! [`InputRouter`]: pinion_core::scene
//! [`WidgetCore`]: pinion_core::WidgetCore
//! [`WidgetA11y`]: pinion_a11y::WidgetA11y
//! [`WidgetView`]: pinion_shell::WidgetView
//! [`WidgetViewTui`]: pinion_tui::WidgetViewTui

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
        flags,
    } = args;

    let optional_forwards = emit_optional_forwards(view_ident, &state, &event, &flags);

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

        impl ::pinion_a11y::WidgetA11y for #view_ident {
            fn access_node(
                state: &#state,
                focused: ::core::option::Option<&str>,
            ) -> ::std::vec::Vec<::pinion_a11y::AccessNode> {
                // R641 §5.16 — bridge trait `&State` to the inherent
                // by-value signature `WidgetCore::State: Copy`
                // guarantees is always sound + matches the clippy
                // `trivially_copy_pass_by_ref` preference for Copy
                // ZST / one-byte enums.
                <#view_ident>::access_node(*state, focused)
            }
        }

        impl ::pinion_shell::WidgetView for #view_ident {
            type Renderer = #renderer;
            fn initial_size() -> (u32, u32) { (#init_w, #init_h) }
        }
    })
}

fn emit_optional_forwards(
    view_ident: &Ident,
    state: &Type,
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
    if flags.contains("apply_composition") {
        out.extend(quote! {
            fn apply_composition(
                scene: &mut ::pinion_core::Scene,
                focused: ::core::option::Option<&str>,
                event: &::pinion_core::input::CompositionEvent,
            ) -> bool {
                <#view_ident>::apply_composition(scene, focused, event)
            }
        });
    }
    if flags.contains("apply_middle_click") {
        out.extend(quote! {
            fn apply_middle_click(
                scene: &mut ::pinion_core::Scene,
                focused: ::core::option::Option<&str>,
                modifiers: ::pinion_core::input::Modifiers,
            ) -> bool {
                <#view_ident>::apply_middle_click(scene, focused, modifiers)
            }
        });
    }
    if flags.contains("focusable_tags") {
        out.extend(quote! {
            fn focusable_tags() -> ::std::vec::Vec<&'static str> {
                <#view_ident>::focusable_tags()
            }
        });
    }
    if flags.contains("fmt_state_log") {
        out.extend(quote! {
            fn fmt_state_log(state: &#state) -> ::std::string::String {
                <#view_ident>::fmt_state_log(state)
            }
        });
    }
    if flags.contains("create_extra_externals") {
        out.extend(quote! {
            fn create_extra_externals() -> ::std::vec::Vec<::pinion_core::ExtraExternal> {
                <#view_ident>::create_extra_externals()
            }
        });
    }
    let _ = state;
    out
}

struct WidgetArgs {
    tag: LitStr,
    state: Type,
    event: Type,
    title: LitStr,
    renderer: Type,
    initial_size: (Expr, Expr),
    external: Expr,
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
                WidgetArg::Flag(name, span) => {
                    if !flags.insert(name.clone()) {
                        return Err(syn::Error::new(span, format!("duplicate '{name}' flag")));
                    }
                }
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
            flags,
        })
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
    Flag(String, proc_macro2::Span),
}

const KNOWN_FLAGS: &[&str] = &[
    "apply_key",
    "keybinding",
    "apply_composition",
    "apply_middle_click",
    "focusable_tags",
    "fmt_state_log",
    "create_extra_externals",
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
                _ => Err(syn::Error::new(
                    key.span(),
                    format!("unknown widget attribute: '{key_str}'"),
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
