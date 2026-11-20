use ts_gen_abi::profile::*;
use ts_gen_abi::protocol;

#[test]
fn defaults_are_all_linear() {
    let p = Profile::default();
    assert_eq!(p, Profile::all_linear());
    assert_eq!(p.vec, Carrier::Linear);
    assert_eq!(p.array, Carrier::Linear);
    assert_eq!(p.str, StrCarrier::Linear(Encoding::Utf8));
    assert_eq!(p.handles, Carrier::Linear);
    assert_eq!(p.memory, Some(MemoryWidth::M32));
    assert_eq!(p.eh, Eh::Modern);
    assert!(p.validation);
    assert!(!p.termination_flag);
    assert!(p.finalizers);
    assert!(p.renames.is_empty());
    assert_eq!(Profile::parse("").unwrap(), p);
}

#[test]
fn all_gc_has_no_memory() {
    let p = Profile::all_gc();
    assert_eq!(
        (p.vec, p.array, p.handles),
        (Carrier::Gc, Carrier::Gc, Carrier::Gc)
    );
    assert_eq!(p.str, StrCarrier::Gc(Encoding::Wtf16));
    assert_eq!(p.memory, None);
    assert!(!p.uses_linear());
    assert_eq!(Profile::parse("all-gc").unwrap(), p);
}

#[test]
fn all_host_is_all_externref() {
    let p = Profile::all_host();
    assert_eq!(
        (p.vec, p.array, p.handles),
        (Carrier::Host, Carrier::Host, Carrier::Host)
    );
    assert_eq!(p.str, StrCarrier::Host);
    assert_eq!(p.memory, None);
    assert!(!p.uses_linear() && !p.finalizers);
    assert_eq!(Profile::parse("all-host").unwrap(), p);
    assert_eq!(Profile::parse(&p.to_string()).unwrap(), p);
    assert!(Profile::parse("all-gc, str = host+utf8").is_err());
}

#[test]
fn settings_override_the_preset() {
    let p = Profile::parse(
        "all-gc # Kotlin\nstr = host, memory = 32\nhandles = linear\neh = legacy\nvalidation = off\ntermination = on\nfinalizers = off\nrename.tw:terminated = __instance_terminated\n",
    )
    .unwrap();
    assert_eq!(p.str, StrCarrier::Host);
    assert_eq!(p.memory, Some(MemoryWidth::M32));
    assert_eq!(p.handles, Carrier::Linear);
    assert_eq!(p.vec, Carrier::Gc);
    assert_eq!(p.eh, Eh::Legacy);
    assert!(!p.validation && p.termination_flag && !p.finalizers);
    assert_eq!(p.name("tw:terminated"), "__instance_terminated");
}

#[test]
fn str_carriers() {
    let cases = [
        ("linear", StrCarrier::Linear(Encoding::Utf8)),
        ("linear+wtf16", StrCarrier::Linear(Encoding::Wtf16)),
        ("gc", StrCarrier::Gc(Encoding::Wtf16)),
        ("gc+utf8", StrCarrier::Gc(Encoding::Utf8)),
        ("gc+wtf16", StrCarrier::Gc(Encoding::Wtf16)),
        ("host", StrCarrier::Host),
    ];
    for (v, want) in cases {
        let p = Profile::parse(&format!("all-gc, memory = 32, str = {v}")).unwrap();
        assert_eq!(p.str, want, "for {v}");
    }
}

#[test]
fn memory64() {
    let p = Profile::parse("memory = 64").unwrap();
    assert_eq!(p.memory, Some(MemoryWidth::M64));
}

#[test]
fn display_reads_back() {
    let mut p = Profile::all_gc();
    p.memory = Some(MemoryWidth::M64);
    p.str = StrCarrier::Linear(Encoding::Wtf16);
    p.termination_flag = true;
    p.renames
        .insert("tw:table".into(), "__wbindgen_externrefs".into());
    for p in [Profile::all_linear(), Profile::all_gc(), p] {
        assert_eq!(Profile::parse(&p.to_string()).unwrap(), p, "{p}");
    }
}

#[test]
fn errors() {
    let cases = [
        (
            "memory = none",
            "profile line 1: a linear category needs `memory = 32` or `memory = 64`",
        ),
        (
            "all-gc\nvec = linear",
            "profile line 2: a linear category needs `memory = 32` or `memory = 64`",
        ),
        (
            "vec = linear, all-gc",
            "profile line 1: the preset `all-gc` must come first",
        ),
        (
            "\nnope",
            "profile line 2: expected `key = value` or a preset (all-linear, all-gc, all-host), found `nope`",
        ),
        ("colour = red", "profile line 1: unknown setting `colour`"),
        (
            "memory = 16",
            "profile line 1: `16` is not a valid `memory`",
        ),
        (
            "str = gc+latin1",
            "profile line 1: `gc+latin1` is not a valid `str`",
        ),
        (
            "validation = yes",
            "profile line 1: `yes` is not a valid `validation`",
        ),
        (
            "rename.tw:free =",
            "profile line 1: `rename.<name> = <new name>`",
        ),
    ];
    for (src, want) in cases {
        assert_eq!(
            Profile::parse(src).unwrap_err().to_string(),
            want,
            "for {src:?}"
        );
    }
}

#[test]
fn renamed_protocol_names() {
    let p = Profile::parse("rename.tw:terminated = __instance_terminated").unwrap();
    assert_eq!(p.name(protocol::TERMINATED), "__instance_terminated");
    assert_eq!(p.name(protocol::REALLOC), "tw:realloc");
}

#[test]
fn results_and_throws() {
    use ts_gen_abi::profile::{Results, Throws};
    let p = Profile::default();
    assert_eq!((p.results, p.throws), (Results::Multi, Throws::Tag));
    let p = Profile::parse("all-host\nresults = single\nthrows = js").unwrap();
    assert_eq!((p.results, p.throws), (Results::Single, Throws::Js));
    assert_eq!(Profile::parse(&p.to_string()).unwrap(), p);
    assert!(Profile::parse("results = one").is_err());
    assert!(Profile::parse("throws = none").is_err());
}

#[test]
fn box_none() {
    assert_eq!(Profile::default().box_none, BoxNone::Null);
    let p = Profile::parse("all-host\nbox-none = unit").unwrap();
    assert_eq!(p.box_none, BoxNone::Unit);
    assert_eq!(Profile::parse(&p.to_string()).unwrap(), p);
    assert!(Profile::parse("box-none = undefined").is_err());
}
