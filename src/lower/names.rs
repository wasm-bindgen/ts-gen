//! Names the IR needs that TypeScript doesn't give: escapes for reserved
//! names, synthesized type and arm names, and overload renames.

use ts_gen_abi::ir as abi;

/// Type-position words a declaration can't take as is (IR.md §2): the
/// built-in names, plus `mut`, which prefixes `*mut T[]` and `mut T[]`.
fn is_reserved_type(name: &str) -> bool {
    abi::BUILTIN_TYPE_NAMES.contains(&name) || name == "mut"
}

/// The IR name of a declaration named `js`: `js` itself, or `js_` when
/// it's reserved, with `@js(name = js)` carrying the JS name.
pub(crate) fn escape_type(js: &str) -> String {
    if is_reserved_type(js) {
        format!("{js}_")
    } else {
        js.to_string()
    }
}

/// The name `escape_type` escaped, for building other names from it.
pub(crate) fn unescape_type(ir: &str) -> &str {
    match ir.strip_suffix('_') {
        Some(js) if is_reserved_type(js) => js,
        _ => ir,
    }
}

/// The IR name of a member named `js`. `new` would read as a constructor.
pub(crate) fn escape_member(js: &str) -> String {
    if js == "new" {
        "new_".into()
    } else {
        js.to_string()
    }
}

/// `@js(name = js)` when the IR name differs.
pub(crate) fn rename(ir: &str, js: &str) -> Option<abi::JsEntry> {
    (ir != js).then(|| abi::JsEntry::Name(js.into()))
}

/// Type parameters for a lib type the input only references.
pub(crate) fn stub_tparams(arity: usize) -> Vec<abi::TypeParam> {
    (0..arity)
        .map(|i| abi::TypeParam {
            name: if arity == 1 {
                "T".into()
            } else {
                format!("T{}", i + 1)
            },
            bound: None,
            default: None,
        })
        .collect()
}

/// `content-type` → `ContentType`, `2d` → `V2d`, `` → `Empty`.
pub(crate) fn pascal(s: &str) -> String {
    let mut out = String::new();
    let mut upper = true;
    for c in s.chars() {
        if c.is_alphanumeric() {
            if upper {
                out.extend(c.to_uppercase());
            } else {
                out.push(c);
            }
            upper = false;
        } else {
            upper = true;
        }
    }
    if out.is_empty() {
        return "Empty".into();
    }
    if out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, 'V');
    }
    out
}

/// `GetText` → `getText`.
pub(crate) fn camel(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) => c.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `name`, or `name2`, `name3`, … , the first that `taken` refuses.
pub(crate) fn unique(name: &str, mut taken: impl FnMut(&str) -> bool) -> String {
    if !taken(name) {
        return name.to_string();
    }
    (2..)
        .map(|n| format!("{name}{n}"))
        .find(|n| !taken(n))
        .unwrap()
}

/// The arm name for a payload of type `t`.
pub(crate) fn arm_name(t: &abi::Type) -> String {
    use abi::Type as T;
    match t {
        T::Unit => "Unit".into(),
        T::UnitBox => "Unit".into(),
        T::Never => "Never".into(),
        T::Scalar(s) => pascal(s.name()),
        T::Str | T::StrBox => "Str".into(),
        T::BigInt | T::BigIntBox => "BigInt".into(),
        T::Any => "Any".into(),
        T::ScalarBox(s) => s.box_name().into(),
        T::Vec(e) | T::VecBox(e) | T::MutVec(e) => format!("{}Vec", arm_name(e)),
        T::Array(e) | T::ArrBox(e) => format!("{}Array", arm_name(e)),
        T::Tuple(_) | T::TupleBox(_) => "Tuple".into(),
        T::Future(..) => "Future".into(),
        T::Nullable(t, _) => arm_name(t),
        T::Fn(_) => "Fn".into(),
        T::View { .. } => "View".into(),
        T::Named(r) => pascal(r.path.segments.last().map_or("Named", |s| s.as_str())),
    }
}

/// A doc comment, unless the JSDoc held only tags.
pub(crate) fn doc(d: &Option<String>) -> Option<String> {
    d.clone().filter(|d| !d.trim().is_empty())
}
