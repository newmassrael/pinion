//! Derive + attribute macros for the §5.20 intent system and the
//! §5.16 widget binding skeleton.
//!
//! ## `#[derive(IntentTag)]`
//!
//! Lifts an enum into the `IntentTag` trait so
//! authors describe widget-emitted intents declaratively. Variant
//! attributes use the `#[tag("name")]` form; the macro derives
//! `const_tag` / `from_intent` / `schema` against the
//! `pinion_core::intent::Intent` envelope.
//!
//! v0 supported variant shapes:
//!
//!   * **Unit variant** — payload type "void"; matches
//!     `IntrospectValue::Null`.
//!   * **Single-field tuple variant** carrying `String`, `i64`, `f64`,
//!     or `bool` — payload type is auto-inferred ("string" / "int" /
//!     "float" / "bool"); matches the corresponding
//!     `IntrospectValue` variant.
//!
//! Multi-field tuple variants and struct variants are intentionally
//! rejected with a clear compile error; richer payload shapes wait on
//! the `IntrospectValue::Object` / `Array` expansion carry-forward
//! noted in §5.20.
//!
//! ## `#[pinion::widget(...)]` (R641 §5.16)
//!
//! Attribute macro that emits the three forwarding trait impls
//! (`WidgetCore` + `WidgetA11y` + `WidgetView`) every visual
//! binding declares, lifting the mechanical wiring (tag / title /
//! associated types / `create_external` factory / `initial_size`)
//! out of every example main.rs while keeping the widget-specific
//! logic (`view` / `read_state` / `event_name` /
//! `access_node`) as inherent methods the macro forwards into. See
//! the `widget` module docs for the full attribute table.
//!
//! ## `#[derive(WidgetStateName)]` / `#[derive(WidgetEventName)]` (SCE-002)
//!
//! Injected onto every sce-generated widget `State` / `Event` enum via
//! `pinion-core`'s `compile_scxml_with_derives` build hook, replacing
//! the per-widget `widget_state_name!` / `widget_event_name!`
//! declarative macros that hand-wrote the `Self ↔ &'static str`
//! statechart-name mapping. The derives reconstruct it from the markers
//! the sce codegen now emits: the `State` enum's `#[default]`
//! SCXML-initial variant (the `from_name_or_default` fallback) and the
//! `Event` enum's `EXTERNALLY_DRIVABLE_EVENTS` associated const (the
//! externally-forgeable subset `from_name` admits, rejecting internal
//! `<raise>` events). Both reject non-unit variants — a statechart
//! state / event carries no payload.
//!
//! The macro emits no `use` statements that would shadow caller
//! symbols — every reference goes through the absolute
//! `::pinion_core::…` / `::pinion_a11y::…` / `::pinion_shell::…` path.
//!
//! These cross-crate references are documented as plain code spans, not
//! intra-doc links: this is a `proc-macro` crate whose only compile
//! dependencies are `syn` / `quote` / `proc-macro2`, so the runtime
//! crates the generated code targets are unreachable to rustdoc here.

mod widget;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::{Data, DeriveInput, Fields, Lit, Meta, Variant, spanned::Spanned};

/// Derive `IntentTag` on an enum.
///
/// See module docs for the supported variant shapes and the payload
/// type-inference table.
#[proc_macro_derive(IntentTag, attributes(tag))]
pub fn derive_intent_tag(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_intent_tag(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// R641 §5.16 — `#[widget(...)]` attribute macro emitting the
/// `WidgetCore` + `WidgetA11y` + `WidgetView` forwarding trio.
///
/// See the `widget` module docs for the full attribute reference and
/// the optional-flag table.
#[proc_macro_attribute]
pub fn widget(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr2: TokenStream2 = attr.into();
    let item2: TokenStream2 = item.into();
    match widget::expand(attr2, item2) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// R644 §5.16 — derive `WidgetTag` on a
/// unit-variant enum.
///
/// Emits `as_tag(&self) -> &'static str` (variant ident converted
/// `PascalCase` → `snake_case` at compile time) and `from_tag(&str)
/// -> Option<Self>` (inverse lookup). Rejects enums whose variants
/// have any fields (tuple or struct shape) — tags carry no
/// payload at the wire level, so the trait insists on the unit
/// shape rather than silently dropping payload bytes.
///
/// ```rust,ignore
/// use pinion_derive::WidgetTag;
///
/// #[derive(Copy, Clone, WidgetTag)]
/// enum Tags { MainBtn, ScrollBar }
///
/// assert_eq!(Tags::MainBtn.as_tag(), "main_btn");
/// assert_eq!(Tags::ScrollBar.as_tag(), "scroll_bar");
/// assert_eq!(Tags::from_tag("main_btn"), Some(Tags::MainBtn));
/// assert_eq!(Tags::from_tag("unknown"), None);
/// ```
#[proc_macro_derive(WidgetTag)]
pub fn derive_widget_tag(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_widget_tag(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_widget_tag(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "WidgetTag can only be derived on enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "WidgetTag derive requires at least one variant",
        ));
    }
    let mut as_tag_arms: Vec<TokenStream2> = Vec::new();
    let mut from_tag_arms: Vec<TokenStream2> = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(
                variant.span(),
                "WidgetTag variants must be unit (tags carry no payload — \
                 tuple / struct variants would silently drop their fields)",
            ));
        }
        let ident = &variant.ident;
        let tag_str = pascal_to_snake_case(&ident.to_string());
        as_tag_arms.push(quote! { Self::#ident => #tag_str });
        from_tag_arms.push(quote! { #tag_str => ::core::option::Option::Some(Self::#ident) });
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::pinion_core::WidgetTag for #name #ty_generics #where_clause {
            fn as_tag(&self) -> &'static str {
                match self {
                    #(#as_tag_arms,)*
                }
            }
            fn from_tag(tag: &str) -> ::core::option::Option<Self> {
                match tag {
                    #(#from_tag_arms,)*
                    _ => ::core::option::Option::None,
                }
            }
        }
    })
}

/// R644 §5.16 — `PascalCase` → `snake_case` converter used by the
/// [`WidgetTag`] derive macro for variant-ident → tag-string
/// conversion at compile time. Inserts `_` before every uppercase
/// ASCII letter after the first character (so `MainBtn` →
/// `main_btn`, `DesignButtonM3` → `design_button_m3`); ASCII digits
/// are treated as lowercase letters (no `_` before them) per the
/// pinion tag convention. Non-ASCII input would be a misuse — tag
/// idents are always ASCII `PascalCase` per
/// [[non-ascii-literal-named-const-escape]] — so the converter is
/// ASCII-only.
fn pascal_to_snake_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 4);
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push(ch);
        }
    }
    out
}

fn expand_intent_tag(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;

    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "IntentTag can only be derived on enums",
        ));
    };

    let mut const_tag_arms: Vec<TokenStream2> = Vec::new();
    let mut from_intent_arms: Vec<TokenStream2> = Vec::new();
    let mut schema_entries: Vec<TokenStream2> = Vec::new();

    for variant in &data.variants {
        let tag = extract_tag_attr(variant)?;
        let kind = classify_variant_payload(variant)?;
        let parts = variant_match_arms(&variant.ident, &tag, kind);
        const_tag_arms.push(parts.const_tag_arm);
        from_intent_arms.push(parts.from_intent_arm);
        schema_entries.push(parts.schema_entry);
    }

    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();

    Ok(quote! {
        impl #impl_generics ::pinion_core::intent::IntentTag for #name #ty_generics #where_clause {
            fn const_tag(&self) -> &'static str {
                match self {
                    #(#const_tag_arms)*
                }
            }

            fn from_intent(intent: &::pinion_core::intent::Intent)
                -> ::core::option::Option<Self>
            {
                match ::core::convert::AsRef::<str>::as_ref(&intent.tag) {
                    #(#from_intent_arms)*
                    _ => ::core::option::Option::None,
                }
            }

            fn schema() -> &'static [(&'static str, &'static str)] {
                &[#(#schema_entries),*]
            }
        }
    })
}

struct VariantParts {
    const_tag_arm: TokenStream2,
    from_intent_arm: TokenStream2,
    schema_entry: TokenStream2,
}

fn variant_match_arms(variant_ident: &syn::Ident, tag: &str, kind: PayloadKind) -> VariantParts {
    match kind {
        PayloadKind::Void => VariantParts {
            const_tag_arm: quote! { Self::#variant_ident => #tag, },
            from_intent_arm: quote! {
                #tag => match &intent.payload {
                    ::pinion_core::external::IntrospectValue::Null
                        => ::core::option::Option::Some(Self::#variant_ident),
                    _ => ::core::option::Option::None,
                },
            },
            schema_entry: quote! { (#tag, "void") },
        },
        PayloadKind::Bool => copy_payload_arm(variant_ident, tag, &quote!(Bool), "bool"),
        PayloadKind::Int => copy_payload_arm(variant_ident, tag, &quote!(Int), "int"),
        PayloadKind::Float => copy_payload_arm(variant_ident, tag, &quote!(Float), "float"),
        PayloadKind::Text => VariantParts {
            const_tag_arm: quote! { Self::#variant_ident(_) => #tag, },
            from_intent_arm: quote! {
                #tag => match &intent.payload {
                    ::pinion_core::external::IntrospectValue::Text(__v)
                        => ::core::option::Option::Some(Self::#variant_ident(__v.clone())),
                    _ => ::core::option::Option::None,
                },
            },
            schema_entry: quote! { (#tag, "string") },
        },
    }
}

fn copy_payload_arm(
    variant_ident: &syn::Ident,
    tag: &str,
    introspect_variant: &TokenStream2,
    schema_name: &str,
) -> VariantParts {
    VariantParts {
        const_tag_arm: quote! { Self::#variant_ident(_) => #tag, },
        from_intent_arm: quote! {
            #tag => match &intent.payload {
                ::pinion_core::external::IntrospectValue::#introspect_variant(__v)
                    => ::core::option::Option::Some(Self::#variant_ident(*__v)),
                _ => ::core::option::Option::None,
            },
        },
        schema_entry: quote! { (#tag, #schema_name) },
    }
}

fn extract_tag_attr(variant: &Variant) -> syn::Result<String> {
    let mut found: Option<String> = None;
    for attr in &variant.attrs {
        if !attr.path().is_ident("tag") {
            continue;
        }
        let Meta::List(meta_list) = &attr.meta else {
            return Err(syn::Error::new(attr.span(), "expected #[tag(\"name\")]"));
        };
        let parsed: Lit = syn::parse2(meta_list.tokens.clone())?;
        let Lit::Str(text) = parsed else {
            return Err(syn::Error::new(
                meta_list.tokens.span(),
                "tag value must be a string literal",
            ));
        };
        if found.is_some() {
            return Err(syn::Error::new(
                attr.span(),
                "duplicate #[tag(...)] on variant",
            ));
        }
        found = Some(text.value());
    }
    found.ok_or_else(|| {
        syn::Error::new(
            variant.span(),
            "every variant must declare #[tag(\"name\")]",
        )
    })
}

#[derive(Clone, Copy)]
enum PayloadKind {
    Void,
    Bool,
    Int,
    Float,
    Text,
}

fn classify_variant_payload(variant: &Variant) -> syn::Result<PayloadKind> {
    match &variant.fields {
        Fields::Unit => Ok(PayloadKind::Void),
        Fields::Unnamed(unnamed) => {
            if unnamed.unnamed.len() != 1 {
                return Err(syn::Error::new(
                    variant.span(),
                    "v0 IntentTag derive supports unit or single-field tuple variants only",
                ));
            }
            let field = unnamed.unnamed.first().expect("len checked above");
            payload_kind_from_type(&field.ty).ok_or_else(|| {
                syn::Error::new(
                    field.ty.span(),
                    "v0 IntentTag payload type must be one of String, i64, f64, bool",
                )
            })
        }
        Fields::Named(_) => Err(syn::Error::new(
            variant.span(),
            "v0 IntentTag derive does not support struct variants",
        )),
    }
}

fn payload_kind_from_type(ty: &syn::Type) -> Option<PayloadKind> {
    let syn::Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if !segment.arguments.is_empty() {
        return None;
    }
    match segment.ident.to_string().as_str() {
        "String" => Some(PayloadKind::Text),
        "i64" => Some(PayloadKind::Int),
        "f64" => Some(PayloadKind::Float),
        "bool" => Some(PayloadKind::Bool),
        _ => None,
    }
}

/// Derive `WidgetStateName` on an sce-generated widget `State` enum: `as_name`
/// maps each variant to its ident string (the SCXML state id); `from_name_or_default`
/// parses it back, falling through to `Self::default()` for an unknown name — the
/// `#[default]`-marked SCXML initial state the sce statechart codegen emits (SCE-002).
/// Injected onto the generated enum via `compile_scxml_with_derives`.
#[proc_macro_derive(WidgetStateName)]
pub fn derive_widget_state_name(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_widget_state_name(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_widget_state_name(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "WidgetStateName can only be derived on enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "WidgetStateName derive requires at least one variant",
        ));
    }
    let mut as_name_arms: Vec<TokenStream2> = Vec::new();
    let mut from_name_arms: Vec<TokenStream2> = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(
                variant.span(),
                "WidgetStateName variants must be unit (a statechart state carries no payload)",
            ));
        }
        let ident = &variant.ident;
        let ident_str = ident.to_string();
        as_name_arms.push(quote! { Self::#ident => #ident_str });
        from_name_arms.push(quote! { #ident_str => Self::#ident });
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::pinion_core::WidgetStateName for #name #ty_generics #where_clause {
            fn as_name(&self) -> &'static str {
                match self { #(#as_name_arms,)* }
            }
            fn from_name_or_default(name: &str) -> Self {
                match name {
                    #(#from_name_arms,)*
                    _ => <Self as ::core::default::Default>::default(),
                }
            }
        }
    })
}

/// Derive `WidgetEventName` on an sce-generated widget `Event` enum: `as_name`
/// maps every variant (external + internal + `Null`) to its ident string; `from_name`
/// parses it back but admits ONLY the externally-drivable variants — those in the
/// `EXTERNALLY_DRIVABLE_EVENTS` associated const the sce statechart codegen emits
/// (SCE-002), rejecting internal `<raise>` events an RPC caller must not forge.
/// Injected via `compile_scxml_with_derives`.
#[proc_macro_derive(WidgetEventName)]
pub fn derive_widget_event_name(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as DeriveInput);
    match expand_widget_event_name(&input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

fn expand_widget_event_name(input: &DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Enum(data) = &input.data else {
        return Err(syn::Error::new(
            input.span(),
            "WidgetEventName can only be derived on enums",
        ));
    };
    if data.variants.is_empty() {
        return Err(syn::Error::new(
            input.span(),
            "WidgetEventName derive requires at least one variant",
        ));
    }
    let mut as_name_arms: Vec<TokenStream2> = Vec::new();
    let mut from_name_arms: Vec<TokenStream2> = Vec::new();
    for variant in &data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(
                variant.span(),
                "WidgetEventName variants must be unit (a statechart event carries no payload)",
            ));
        }
        let ident = &variant.ident;
        let ident_str = ident.to_string();
        as_name_arms.push(quote! { Self::#ident => #ident_str });
        from_name_arms.push(quote! { #ident_str => Self::#ident });
    }
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::pinion_core::WidgetEventName for #name #ty_generics #where_clause {
            fn as_name(&self) -> &'static str {
                match self { #(#as_name_arms,)* }
            }
            fn from_name(name: &str) -> ::core::option::Option<Self> {
                let candidate = match name {
                    #(#from_name_arms,)*
                    _ => return ::core::option::Option::None,
                };
                if Self::EXTERNALLY_DRIVABLE_EVENTS.contains(&candidate) {
                    ::core::option::Option::Some(candidate)
                } else {
                    ::core::option::Option::None
                }
            }
            // R1564 — the same const `from_name` gates on, rendered as the
            // names a caller may send. Derived from that const rather than
            // from the variant list, so the vocabulary a refusal advertises
            // and the vocabulary `from_name` admits cannot drift apart.
            fn drivable_names() -> ::std::vec::Vec<&'static str> {
                Self::EXTERNALLY_DRIVABLE_EVENTS
                    .iter()
                    .map(::pinion_core::WidgetEventName::as_name)
                    .collect()
            }
        }
    })
}
