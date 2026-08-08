//! R1539 §5.7 §5.12 §2 #7 — the published wire census is true of the types.
//!
//! [`pinion_rpc::wire_census::WIRE_TYPES`] declares what every serialized type
//! puts on the wire. A declaration nothing checks is a comment: R1538 grew
//! `FrameTimingsMirror` by one field and the only thing in the repo that
//! noticed was a demo, 44 minutes into CI, after the push.
//!
//! This test parses the crate's OWN source and asserts the census matches it —
//! so the same edit fails here, in `pinion-rpc`'s unit gate, in the round that
//! makes it. When it fails it also names the demos that assert on the changed
//! field names, because the local gate runs the demos a round TOUCHED and the
//! ones that break are the ones it did not.
//!
//! The parser is source-text, the same trade `methods.rs`'s
//! `catalog_matches_dispatch_match_arms` already makes: robust for the shapes
//! this crate uses, and **loud** rather than lenient when it meets one it does
//! not model — an unmodelled `#[serde]` attribute panics instead of being
//! silently dropped, because a census that quietly skips what it cannot read
//! is worse than none.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;

use pinion_rpc::wire_census::{WIRE_TYPES, WireShape, WireTy};

// ── source access ───────────────────────────────────────────────────────────

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` in this crate's `src/`, read at test time.
///
/// Deliberately NOT an `include_str!` list: that list would itself be a census
/// that a new module could silently miss, which is the exact failure mode this
/// test exists to remove.
fn sources() -> Vec<(String, String)> {
    let dir = crate_dir().join("src");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .expect("pinion-rpc/src is readable")
        .map(|e| e.expect("dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .map(|p| {
            let name = p
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            (name, std::fs::read_to_string(&p).expect("source is UTF-8"))
        })
        .collect();
    out.sort();
    assert!(
        out.len() > 20,
        "expected the whole crate, got {}",
        out.len()
    );
    out
}

/// The body of the `{…}` block at or after `from`, and its closing index.
fn braced(src: &str, from: usize) -> (&str, usize) {
    let open = from + src[from..].find('{').expect("a brace follows");
    let mut depth = 0usize;
    for (i, b) in src.bytes().enumerate().skip(open) {
        match b {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (&src[open + 1..i], i);
                }
            }
            _ => {}
        }
    }
    panic!("unbalanced braces from byte {from}");
}

// ── parsed model ────────────────────────────────────────────────────────────

/// A field as written in Rust: wire name, may-be-absent, Rust type text.
type RawField = (String, bool, String);

#[derive(Debug, Clone, PartialEq, Eq)]
enum Parsed {
    Object(Vec<RawField>),
    Enum(Vec<String>),
    Union(String, Vec<(String, Vec<RawField>)>),
    Scalar(Vec<String>),
}

/// The string a `#[serde(key = "…")]` attribute carries.
fn attr_value<'a>(attrs: &'a str, key: &str) -> Option<&'a str> {
    let at = attrs.find(key)?;
    let rest = &attrs[at + key.len()..];
    let q = rest.find('"')?;
    let end = rest[q + 1..].find('"')?;
    Some(&rest[q + 1..q + 1 + end])
}

fn apply_rename_all(container_attrs: &str, ident: &str) -> String {
    match attr_value(container_attrs, "rename_all") {
        None => ident.to_owned(),
        Some("lowercase") => ident.to_lowercase(),
        Some("snake_case") => {
            let mut out = String::new();
            for (i, c) in ident.char_indices() {
                if c.is_uppercase() && i != 0 {
                    out.push('_');
                }
                out.extend(c.to_lowercase());
            }
            out
        }
        Some(other) => panic!("unmodelled rename_all = {other:?} — teach this parser first"),
    }
}

/// Parse `pub name: Type,` declarations, carrying each one's `#[serde]` attrs.
///
/// Character-driven rather than line-driven, because BOTH shapes occur here
/// and a line-oriented reader gets each of them wrong in a way that looks like
/// success: a multi-line `#[serde(…)]` drops every attribute after the first
/// line (so a `skip_serializing_if` on its own line vanishes and a required
/// key reads as optional), and a one-line variant body — `Offset { x: i32, y:
/// i32 }` — reads as a single field named `x` of type `i32, y: i32`. The first
/// draft of this parser did both, and the census caught it rather than the
/// other way round.
fn parse_fields(body: &str, require_pub: bool) -> Vec<RawField> {
    let mut out = Vec::new();
    let mut attrs = String::new();
    let mut decl = String::new();
    let mut depth = 0i32;
    let bytes: Vec<char> = body.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        // `#[…]` — consume the whole balanced attribute, however many lines.
        if depth == 0 && c == '#' && bytes.get(i + 1) == Some(&'[') {
            let mut d = 0i32;
            let start = i;
            while i < bytes.len() {
                match bytes[i] {
                    '[' => d += 1,
                    ']' => {
                        d -= 1;
                        if d == 0 {
                            i += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                i += 1;
            }
            attrs.extend(bytes[start..i].iter());
            continue;
        }
        // `//` and `///` — skip to end of line.
        if c == '/' && bytes.get(i + 1) == Some(&'/') {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        match c {
            '<' | '(' | '[' | '{' => depth += 1,
            '>' | ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
        if c == ',' && depth == 0 {
            push_field(&mut out, &decl, &attrs, require_pub);
            decl.clear();
            attrs.clear();
        } else {
            decl.push(c);
        }
        i += 1;
    }
    push_field(&mut out, &decl, &attrs, require_pub);
    out
}

/// Record one `name: Type` declaration, if that is what `decl` holds.
fn push_field(out: &mut Vec<RawField>, decl: &str, attrs: &str, require_pub: bool) {
    let t = decl.trim();
    let d = match t.strip_prefix("pub ") {
        Some(d) => d.trim(),
        None if require_pub => return,
        None => t,
    };
    let Some((name, ty)) = d.split_once(':') else {
        return;
    };
    let name = name.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return;
    }
    assert!(
        !attrs.contains("flatten") && !attrs.contains("serde(skip)"),
        "field `{name}` carries a serde attribute this census does not model \
         ({attrs}) — teach the parser and the census before using it"
    );
    let wire = attr_value(attrs, "rename").unwrap_or(name).to_owned();
    out.push((
        wire,
        attrs.contains("skip_serializing_if"),
        ty.trim().to_owned(),
    ));
}

/// Variant identifiers at an enum body's base depth, with payload text.
fn enum_variants(body: &str) -> Vec<(String, Option<String>, String)> {
    let mut out = Vec::new();
    let mut attrs = String::new();
    let mut depth = 0i32;
    for line in body.lines() {
        let t = line.trim();
        if depth == 0 {
            if t.starts_with("#[") {
                attrs.push_str(t);
            } else if !t.starts_with("//") && !t.is_empty() {
                let ident: String = t
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() {
                    let after = t[ident.len()..].trim_start();
                    let payload = if after.starts_with('{') {
                        Some(String::new())
                    } else {
                        after.strip_prefix('(').map(|inner| {
                            inner.split(')').next().expect("a paren closes").to_owned()
                        })
                    };
                    out.push((ident, payload, attrs.clone()));
                }
                attrs.clear();
            }
        }
        depth += i32::try_from(t.matches('{').count()).expect("small")
            - i32::try_from(t.matches('}').count()).expect("small");
    }
    out
}

fn primitive(rust: &str) -> Option<&'static str> {
    Some(match rust.rsplit("::").next().unwrap_or(rust).trim() {
        "u8" | "u16" | "u32" | "u64" | "usize" | "i8" | "i16" | "i32" | "i64" | "isize" => {
            "integer"
        }
        "f32" | "f64" => "number",
        "String" | "str" | "char" => "string",
        "bool" => "boolean",
        _ => return None,
    })
}

fn parse_enum(body: &str, attrs: &str) -> Parsed {
    let variants = enum_variants(body);
    if attrs.contains("untagged") {
        return Parsed::Scalar(
            variants
                .iter()
                .map(|(_, payload, _)| match payload.as_deref() {
                    None => "null".to_owned(),
                    Some(inner) => primitive(inner).unwrap_or("any").to_owned(),
                })
                .collect(),
        );
    }
    if let Some(tag) = attr_value(attrs, "tag") {
        let mut arms = Vec::new();
        let mut rest = body;
        for (ident, payload, vattrs) in &variants {
            let named = apply_rename_all(
                attrs,
                attr_value(vattrs, "rename").unwrap_or(ident.as_str()),
            );
            let fields = match payload.as_deref() {
                None => Vec::new(),
                Some("") => {
                    let idx = rest
                        .find(ident.as_str())
                        .expect("the variant is in the body");
                    let (vb, end) = braced(rest, idx);
                    rest = &rest[end..];
                    parse_fields(vb, false)
                }
                Some(_) => panic!(
                    "internally-tagged enum variant `{ident}` carries a tuple payload, \
                     which serde cannot represent"
                ),
            };
            arms.push((named, fields));
        }
        return Parsed::Union(tag.to_owned(), arms);
    }
    Parsed::Enum(
        variants
            .iter()
            .map(|(ident, payload, vattrs)| {
                assert!(
                    payload.is_none(),
                    "enum variant `{ident}` carries data but the enum is neither \
                     tagged nor untagged — teach the census what it serializes as"
                );
                apply_rename_all(
                    attrs,
                    attr_value(vattrs, "rename").unwrap_or(ident.as_str()),
                )
            })
            .collect(),
    )
}

/// Every `Serialize` type this crate declares, and every `pub type` alias.
fn parse_crate() -> (BTreeMap<String, Parsed>, BTreeMap<String, String>) {
    let mut types = BTreeMap::new();
    let mut aliases = BTreeMap::new();
    for (file, src) in sources() {
        for line in src.lines() {
            if let Some(rest) = line.trim().strip_prefix("pub type ") {
                if let Some((lhs, rhs)) = rest.split_once('=') {
                    aliases.insert(
                        lhs.trim().to_owned(),
                        rhs.trim().trim_end_matches(';').to_owned(),
                    );
                }
            }
        }
        let mut at = 0usize;
        while let Some(rel) = src[at..].find("#[derive(") {
            let start = at + rel;
            let close = start + src[start..].find(")]").expect("the derive closes") + 2;
            let derives = &src[start..close];
            at = close;
            if !derives.contains("Serialize") {
                continue;
            }
            let mut attrs = String::new();
            let mut cur = close;
            let head = loop {
                let nl = src[cur..].find('\n').map_or(src.len(), |n| cur + n);
                let line = src[cur..nl].trim();
                if line.is_empty() || line.starts_with("//") {
                    cur = nl + 1;
                } else if line.starts_with("#[") {
                    attrs.push_str(line);
                    cur = nl + 1;
                } else {
                    break line.to_owned();
                }
            };
            let Some(kw) = ["pub struct ", "pub enum "]
                .into_iter()
                .find(|k| head.starts_with(k))
            else {
                continue;
            };
            if !head.contains('{') {
                continue; // tuple / unit struct — not an object on the wire
            }
            let name = head[kw.len()..]
                .split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .next()
                .expect("a name follows the keyword")
                .to_owned();
            let (body, end) = braced(&src, cur);
            at = end;
            let parsed = if kw == "pub struct " {
                Parsed::Object(parse_fields(body, true))
            } else {
                parse_enum(body, &attrs)
            };
            assert!(
                types.insert(name.clone(), parsed).is_none(),
                "two Serialize types are named `{name}` (the second is in {file})"
            );
        }
    }
    (types, aliases)
}

// ── rendering: one normalized text form both sides produce ──────────────────

/// What a censused type serializes AS, so a field carrying it can be typed.
fn json_ty_of(parsed: &Parsed) -> &'static str {
    match parsed {
        Parsed::Object(_) | Parsed::Union(..) => "object",
        Parsed::Enum(_) => "string",
        Parsed::Scalar(_) => "any",
    }
}

/// `(optional, json type, censused type at this key)` for a Rust type text.
fn classify(
    rust: &str,
    types: &BTreeMap<String, Parsed>,
    aliases: &BTreeMap<String, String>,
) -> (bool, String, Option<String>) {
    fn resolve(t: &str, aliases: &BTreeMap<String, String>) -> String {
        let mut t = t.trim().trim_end_matches(',').trim().to_owned();
        if let Some(r) = t.strip_prefix('&') {
            t = r.trim_start().to_owned();
            if t.starts_with('\'') {
                t = t
                    .split_once(char::is_whitespace)
                    .map_or(t.clone(), |(_, r)| r.trim().to_owned());
            }
        }
        let mut seen = 0;
        while let Some(next) = aliases.get(&t) {
            t = next.clone();
            seen += 1;
            assert!(seen < 16, "alias cycle at {t}");
        }
        t
    }
    fn inner<'a>(t: &'a str, outer: &str) -> Option<&'a str> {
        t.strip_prefix(outer)?.strip_suffix('>')
    }

    let t = resolve(rust, aliases);
    // R1610 — `Patch<T>` is this crate's three-state wire patch: absent leaves
    // the axis alone, an explicit null clears it, a value sets it. It carries
    // no `#[derive(Serialize)]` (both halves are hand-written, which is where
    // the three states live), so the scan below never sees it and every field
    // using one would publish as `any` — telling an agent nothing about the
    // very axis whose three-ness the type exists for. Unwrapping it here is a
    // STRUCTURAL fact about a local type, not a hand-kept name-to-type table:
    // it is exactly `Option<T>` on the wire, and additionally nullable.
    let (patched, t) = match inner(&t, "Patch<") {
        Some(i) => (true, resolve(i, aliases)),
        None => (false, t),
    };
    let _ = patched;
    let (optional, t) = match inner(&t, "Option<") {
        Some(i) => (true, resolve(i, aliases)),
        None => (false, t),
    };
    let optional = optional || patched;
    let elem = inner(&t, "Vec<").map(|i| resolve(i, aliases)).or_else(|| {
        t.strip_prefix('[')
            .and_then(|i| i.strip_suffix(']'))
            .map(|i| resolve(i, aliases))
    });
    if let Some(e) = elem {
        let of = types.contains_key(&e).then_some(e);
        return (optional, "array".to_owned(), of);
    }
    if t.starts_with('(') {
        return (optional, "array".to_owned(), None);
    }
    if let Some(p) = types.get(&t) {
        return (optional, json_ty_of(p).to_owned(), Some(t));
    }
    let ty = primitive(&t).unwrap_or("any").to_owned();
    (optional, ty, None)
}

/// R1610 — does this field carry a [`Patch`], which is BOTH absent-able and
/// null-able? The two facts are separate everywhere else in this census (a
/// `skip_serializing_if` makes a key absent; an `Option` makes it null), and a
/// patch is the one shape that is genuinely both, which is the whole reason it
/// exists as a type.
fn is_patch(rust: &str) -> bool {
    rust.trim()
        .trim_end_matches(',')
        .trim()
        .starts_with("Patch<")
}

/// `name ty` plus `?` when the key may be absent and `|null` when it is always
/// present but may carry `null` — the two are DIFFERENT answers, so the
/// rendering keeps them apart rather than collapsing both to "maybe".
fn render_field(
    prefix: &str,
    name: &str,
    optional: bool,
    nullable: bool,
    ty: &str,
    of: Option<&str>,
) -> String {
    let mut s = format!("{prefix}.{name} {ty}");
    if optional {
        s.push('?');
    }
    if nullable {
        s.push_str("|null");
    }
    if let Some(o) = of {
        let _ = write!(s, " -> {o}");
    }
    s
}

fn render_parsed(
    name: &str,
    parsed: &Parsed,
    types: &BTreeMap<String, Parsed>,
    aliases: &BTreeMap<String, String>,
) -> Vec<String> {
    let mut out = Vec::new();
    match parsed {
        Parsed::Object(fields) => {
            out.push(format!("{name} object"));
            for (f, skipped, rust) in fields {
                let (is_option, ty, of) = classify(rust, types, aliases);
                out.push(render_field(
                    name,
                    f,
                    *skipped,
                    (is_option && !skipped) || is_patch(rust),
                    &ty,
                    of.as_deref(),
                ));
            }
        }
        Parsed::Enum(values) => out.push(format!("{name} enum [{}]", values.join("|"))),
        Parsed::Union(tag, arms) => {
            out.push(format!("{name} union tag={tag}"));
            for (variant, fields) in arms {
                let head = format!("{name}#{variant}");
                out.push(head.clone());
                for (f, skipped, rust) in fields {
                    let (is_option, ty, of) = classify(rust, types, aliases);
                    out.push(render_field(
                        &head,
                        f,
                        *skipped,
                        (is_option && !skipped) || is_patch(rust),
                        &ty,
                        of.as_deref(),
                    ));
                }
            }
        }
        Parsed::Scalar(tys) => out.push(format!("{name} scalar [{}]", tys.join("|"))),
    }
    out
}

fn ty_name(ty: WireTy) -> &'static str {
    match ty {
        WireTy::Integer => "integer",
        WireTy::Number => "number",
        WireTy::String => "string",
        WireTy::Boolean => "boolean",
        WireTy::Array => "array",
        WireTy::Object => "object",
        WireTy::Null => "null",
        WireTy::Any => "any",
    }
}

fn render_declared(t: &pinion_rpc::wire_census::WireType) -> Vec<String> {
    let mut out = Vec::new();
    match t.shape {
        WireShape::Object { fields } => {
            out.push(format!("{} object", t.name));
            for f in fields {
                out.push(render_field(
                    t.name,
                    f.name,
                    f.optional,
                    f.nullable,
                    ty_name(f.ty),
                    f.of,
                ));
            }
        }
        WireShape::Enum { values } => {
            out.push(format!("{} enum [{}]", t.name, values.join("|")));
        }
        WireShape::Union { tag, variants } => {
            out.push(format!("{} union tag={tag}", t.name));
            for v in variants {
                let head = format!("{}#{}", t.name, v.name);
                out.push(head.clone());
                for f in v.fields {
                    out.push(render_field(
                        &head,
                        f.name,
                        f.optional,
                        f.nullable,
                        ty_name(f.ty),
                        f.of,
                    ));
                }
            }
        }
        WireShape::Scalar { types } => {
            let names: Vec<&str> = types.iter().copied().map(ty_name).collect();
            out.push(format!("{} scalar [{}]", t.name, names.join("|")));
        }
    }
    out
}

// ── the consumers a shape change breaks ─────────────────────────────────────

/// Demos that assert on any of `fields`, most-affected first.
///
/// The reason this test exists is that the local gate runs the demos a round
/// TOUCHED, and the ones a wire change breaks are exactly the ones it did not.
/// Naming them turns "something else may assert this" into a list.
///
/// The match is on the QUOTED key (`"nodes_total"`), because that is how a
/// demo names a wire field; a bare substring match makes a change to a field
/// called `tag` or `name` report almost every demo in the tree, which is true
/// and useless. Ranked by how many of the changed fields a demo names, so the
/// head of the list is the demo most likely to be the one that breaks.
fn demo_consumers(fields: &BTreeSet<String>) -> Vec<(usize, String)> {
    let dir = crate_dir().join("../../tools/demos");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut hits: Vec<(usize, String)> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "py"))
        .filter_map(|p| {
            let body = std::fs::read_to_string(&p).ok()?;
            let n = fields
                .iter()
                .filter(|f| body.contains(&format!("\"{f}\"")))
                .count();
            if n == 0 {
                return None;
            }
            Some((n, p.file_name()?.to_string_lossy().into_owned()))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    hits
}

// ── the gates ───────────────────────────────────────────────────────────────

#[test]
fn census_matches_the_types() {
    let (parsed, aliases) = parse_crate();

    let declared_names: BTreeSet<&str> = WIRE_TYPES.iter().map(|t| t.name).collect();
    let parsed_names: BTreeSet<&str> = parsed.keys().map(String::as_str).collect();
    assert_eq!(
        parsed_names, declared_names,
        "WIRE_TYPES must list EVERY Serialize type this crate declares and no \
         others. A type only this crate's source knows about is a response \
         shape no agent can discover; a type only WIRE_TYPES knows about is a \
         promise nothing keeps."
    );

    let mut wrong: Vec<(&str, Vec<String>, Vec<String>)> = Vec::new();
    let mut changed: BTreeSet<String> = BTreeSet::new();
    for t in WIRE_TYPES {
        let from_source = render_parsed(t.name, &parsed[t.name], &parsed, &aliases);
        let from_census = render_declared(t);
        if from_source != from_census {
            for line in from_source.iter().chain(from_census.iter()) {
                if let Some((head, _)) = line.split_once(' ') {
                    if let Some((_, field)) = head.rsplit_once('.') {
                        changed.insert(field.to_owned());
                    }
                }
            }
            wrong.push((t.name, from_source, from_census));
        }
    }
    assert!(wrong.is_empty(), "{}", report(&wrong, &changed));
}

fn report(wrong: &[(&str, Vec<String>, Vec<String>)], changed: &BTreeSet<String>) -> String {
    let mut msg = String::from(
        "\nthe wire census disagrees with the types it describes.\n\
         `<` is what the source says, `>` is what WIRE_TYPES claims.\n\n",
    );
    for (name, src, census) in wrong {
        let _ = writeln!(msg, "  {name}:");
        for line in src {
            if !census.contains(line) {
                let _ = writeln!(msg, "    < {line}");
            }
        }
        for line in census {
            if !src.contains(line) {
                let _ = writeln!(msg, "    > {line}");
            }
        }
    }
    let consumers = demo_consumers(changed);
    if consumers.is_empty() {
        msg.push_str(
            "\nNo demo names these fields — but a response shape is a PUBLISHED \
             contract, so update WIRE_TYPES deliberately, not to make this pass.\n",
        );
    } else {
        let _ = write!(
            msg,
            "\n{} demo(s) assert on the affected field names, and the local gate \
             does NOT run them unless this round touched them. Most-affected \
             first — run these before pushing:\n",
            consumers.len()
        );
        for (n, c) in consumers.iter().take(12) {
            let _ = writeln!(msg, "    python3 tools/demos/{c}   ({n} field(s))");
        }
        if consumers.len() > 12 {
            let _ = writeln!(msg, "    … and {} more", consumers.len() - 12);
        }
    }
    msg
}

#[test]
fn census_is_sorted_and_unique() {
    let names: Vec<&str> = WIRE_TYPES.iter().map(|t| t.name).collect();
    let mut sorted = names.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted, names, "WIRE_TYPES must be sorted by name + unique");
}

#[test]
fn every_reference_resolves() {
    // Through `wire_type`, not a name set built here: that function is what a
    // Rust consumer resolves an `of` with, so the gate exercises the shipped
    // resolver rather than a second copy of its logic that could agree with
    // the census while the real one did not.
    let mut refs: Vec<(&str, &str, &str)> = Vec::new();
    for t in WIRE_TYPES {
        let fields: Vec<(&str, &[pinion_rpc::wire_census::WireField])> = match t.shape {
            WireShape::Object { fields } => vec![(t.name, fields)],
            WireShape::Union { variants, .. } => {
                variants.iter().map(|v| (v.name, v.fields)).collect()
            }
            WireShape::Enum { .. } | WireShape::Scalar { .. } => Vec::new(),
        };
        for (owner, fs) in fields {
            for f in fs {
                if let Some(of) = f.of {
                    refs.push((t.name, owner, of));
                    assert!(
                        pinion_rpc::wire_census::wire_type(of).is_some(),
                        "{}.{} references type `{of}`, which WIRE_TYPES does not \
                         define — a `$ref` an agent cannot resolve",
                        t.name,
                        f.name
                    );
                }
                assert!(
                    !matches!(f.ty, WireTy::Object) || f.of.is_some(),
                    "{}.{} is an object with no named type — an agent reading \
                     this learns only that a `{{}}` is there",
                    t.name,
                    f.name
                );
            }
        }
    }
    assert!(
        refs.len() > 10,
        "the census should nest — got {} references",
        refs.len()
    );
}

#[test]
fn the_census_describes_itself() {
    // §2 #7 applied to the description: the types that carry the census are
    // themselves on the wire, so they must be in it. If this ever fails, an
    // agent can read every response shape EXCEPT the shape of the answer that
    // told it the shapes.
    for own in [
        "WireField",
        "WireVariant",
        "WireType",
        "WireShape",
        "WireTy",
    ] {
        assert!(
            pinion_rpc::wire_census::wire_type(own).is_some(),
            "the census must describe its own type `{own}`"
        );
    }
}
