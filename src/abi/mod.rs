//! The ts-gen IR of a `.d.ts` ([`build_ir`]): one binding per member, with
//! wasm-level types.
//!
//! The model and its text format live in the `ts-gen-abi` crate; this
//! module lowers a `.d.ts` to it. Two consumers are in ts-gen:
//!
//! * [`wat`] prints the IR's bindings as a WAT import template
//!   (`--target wat`), for a language written in WAT by hand.
//! * [`embed`] implements the bindings for a given language module: a
//!   `bindings.wasm` built on a fixed set of JS primitives, a
//!   `wrapper.wasm` that presents the module's provides at their JS
//!   paths, and the `builtins.js` those two import.

pub mod embed;
mod lower;
pub mod wat;

use crate::codegen::GenerateOptions;
use crate::context::GlobalContext;
use crate::ir::Module;
use crate::provides::ProvidedFn;

pub use ts_gen_abi::ir::Ir;
pub use ts_gen_abi::{
    OwnerRef, WasmType, BINDINGS_SPECIFIER, BUILTINS_SPECIFIER, STRING_CONSTANTS, UNDEFINED_GLOBAL,
};

/// The IR of the declarations the Rust bindings emit for `module` under
/// `options`: one member per Rust binding, under its Rust name, with the
/// bindings that only present another in Rust left out.
pub fn build_ir(
    module: &Module,
    gctx: &GlobalContext,
    options: &GenerateOptions,
    provided: &[ProvidedFn],
) -> Ir {
    lower::build(module, gctx, options, provided)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::ModuleContext;
    use ts_gen_abi::ir::{Head as H, Import, Literal, MemberKind, Type};

    /// Every declared module is selected, as `--export <specifier>` would.
    fn exports(module: &Module, gctx: &GlobalContext) -> GenerateOptions {
        let exports = module
            .types
            .iter()
            .filter_map(|&id| match &gctx.get_type(id).module_context {
                ModuleContext::Module(m) => Some(crate::codegen::ExportSpec::Module(m.to_string())),
                ModuleContext::Global => None,
            })
            .chain([crate::codegen::ExportSpec::File("<input>".into())])
            .collect();
        GenerateOptions::new().exports(exports)
    }

    /// The first import named `name`, its own name or an alias, from any
    /// module.
    fn find<'a>(ir: &'a Ir, name: &str) -> Option<Import<'a>> {
        ir.imports().find(|i| i.names.iter().any(|n| n == name))
    }

    fn imp<'a>(ir: &'a Ir, name: &str) -> Import<'a> {
        find(ir, name).unwrap_or_else(|| panic!("no import {name}"))
    }

    /// The import a language module imports as `name` from `module`.
    fn imp_in<'a>(ir: &'a Ir, module: &str, name: &str) -> Import<'a> {
        ir.imports()
            .find(|i| i.specifier() == module && i.names.iter().any(|n| n == name))
            .unwrap_or_else(|| panic!("no import {module} {name}"))
    }

    fn result(ir: &Ir, name: &str) -> Option<WasmType> {
        imp(ir, name).member.result.wasm()
    }

    /// The wire type of a parameter, receiver excluded.
    fn param(ir: &Ir, name: &str, i: usize) -> WasmType {
        imp(ir, name).member.params[i].ty.wasm().unwrap()
    }

    fn declared(module: Option<&str>, path: &str) -> OwnerRef {
        OwnerRef {
            module: module.map(String::from),
            path: path.into(),
        }
    }

    fn owner(ty: &Type) -> OwnerRef {
        match head(ty) {
            H::Named(n) => n.name.clone(),
            other => panic!("not named: {other:?}"),
        }
    }

    const NUL: WasmType = WasmType::ExternRef {
        null: true,
        undefined: false,
    };
    const UND: WasmType = WasmType::ExternRef {
        null: false,
        undefined: true,
    };

    #[test]
    fn module_declarations_carry_their_specifier() {
        let ir = ir(r#"
            declare function fetch(input: string): Promise<Response>;
            declare module "cloudflare:sockets" {
                function connect(address: string): Socket;
                class Socket { startTls(): Socket; }
            }
        "#);
        assert_eq!(imp(&ir, "fetch").module, None);
        assert_eq!(imp(&ir, "connect").module, Some("cloudflare:sockets"));
        assert_eq!(
            imp(&ir, "Socket.start_tls").module,
            Some("cloudflare:sockets")
        );
        let mut modules: Vec<&str> = ir.imports().filter_map(|i| i.module).collect();
        modules.dedup();
        assert_eq!(modules, ["cloudflare:sockets"]);
    }

    #[test]
    fn global_interface_collapses_into_module_constructor() {
        let ir = ir(r#"
            interface Email { readonly from: string; }
            declare module "cloudflare:email" {
                let _Email: { new (from: string): Email };
                export { _Email as Email };
            }
        "#);
        let new = imp_in(&ir, "cloudflare:email", "Email.new");
        assert_eq!(new.member.kind, MemberKind::Constructor);
        assert_eq!(new.module, Some("cloudflare:email"));
        // The global instance interface collapses into the module's
        // constructor class, so its members are the class's.
        assert_eq!(imp(&ir, "Email.from").module, Some("cloudflare:email"));
    }

    #[test]
    fn nullability_sees_through_aliases() {
        let ir = ir(r#"
            type MaybeStr = string | null;
            type Alias2 = MaybeStr;
            type Body = Blob | MaybeStr;
            type Plain = string;
            type Gen<T> = T | null;
            declare class Blob {}
            declare function a(): MaybeStr;
            declare function b(): Alias2;
            declare function c(): Body;
            declare function d(): Plain;
            declare function e(x: Alias2): void;
            declare function g(): Gen<Blob>;
            declare function clone<T>(value: T): T;
            declare function unknownName(): NotDeclaredAnywhere;
            declare class Event<T = any> { readonly detail: T; }
        "#);
        let result = |n: &str| result(&ir, n).unwrap();
        assert!(result("a").admits_null());
        assert!(result("b").admits_null());
        assert!(result("c").admits_null());
        assert_eq!(result("d"), WasmType::JS_NON_NULL);
        // The `null` arm of a parameter is a binding of its own.
        assert_eq!(param(&ir, "e", 0), WasmType::JS_NON_NULL);
        assert!(param(&ir, "e_with_null", 0).admits_null());
        assert!(result("g").admits_null(), "generic alias instantiation");
        assert_eq!(result("clone"), WasmType::JS, "type parameter");
        assert_eq!(param(&ir, "clone", 0), WasmType::JS);
        assert_eq!(result("unknown_name"), WasmType::JS, "unresolved name");
        assert_eq!(result("Event.detail"), WasmType::JS, "class type parameter");
    }

    #[test]
    fn scalars_through_aliases() {
        let ir = ir(r#"
            type Millis = number;
            type Stamp = Millis;
            type Flag = boolean;
            type Big = bigint;
            type Nothing = void;
            type Id<T> = T;
            declare function now(): Stamp;
            declare function enabled(): Flag;
            declare function big(): Big;
            declare function reset(): Nothing;
            declare function wrapped(): Id<number>;
            declare function wait(ms: Millis): void;
            declare function same<T>(value: T): T;
        "#);
        let result = |n: &str| result(&ir, n);
        assert_eq!(result("now"), Some(WasmType::F64), "alias of an alias");
        assert_eq!(result("enabled"), Some(WasmType::I32));
        assert_eq!(result("big"), Some(WasmType::I64));
        assert_eq!(result("reset"), None, "alias of void");
        assert_eq!(result("wrapped"), Some(WasmType::F64), "type argument");
        assert_eq!(param(&ir, "wait", 0), WasmType::F64);
        assert_eq!(result("same"), Some(WasmType::JS), "type parameter");
    }

    #[test]
    fn namespaced_types_keep_their_path() {
        let ir = ir(r#"
            declare class Header { a(): void; }
            declare namespace TailStream { class Header { b(): void; } }
            declare namespace WebAssembly { class Module { constructor(bytes: ArrayBuffer); static exports(m: Module): string[]; } }
        "#);
        let names: Vec<String> = ir.imports().map(|i| i.name().to_string()).collect();
        assert_eq!(
            names,
            [
                "Header.a",
                "TailStream.Header.b",
                "WebAssembly.Module.new",
                "WebAssembly.Module.exports"
            ]
        );
        let ctor = imp(&ir, "WebAssembly.Module.new").member;
        assert_eq!(owner(&ctor.result), OwnerRef::global("WebAssembly.Module"));
        let exports = imp(&ir, "WebAssembly.Module.exports").member;
        assert_eq!(
            owner(&exports.params[0].ty),
            OwnerRef::global("WebAssembly.Module")
        );
    }

    #[test]
    fn overload_variants_use_the_rust_suffixes() {
        let ir = ir(r#"
            declare class K {
                constructor();
                constructor(init: string);
                m(a: string): void;
                m(a: number): void;
                static s(x: string): void;
                static s(x: string, y: number): void;
            }
        "#);
        let names: Vec<String> = ir.imports().map(|i| i.name().to_string()).collect();
        assert_eq!(
            names,
            [
                "K.new",
                "K.new_with_init",
                "K.m",
                "K.m_with_f64",
                "K.s",
                "K.s_with_y"
            ]
        );
    }

    #[test]
    fn nullability_through_type_params_qualified_paths_and_generic_aliases() {
        let ir = ir(r#"
            declare class Headers { constructor(); }
            declare class Gen<T> {
                optT(): T | undefined;
                nullableT(): T | null;
                plainT(): T;
            }
            declare namespace NS { type Inner = Headers | null; class K { constructor(); } }
            type Gen2<T> = T | null;
            type Id<T> = T;
            interface Uses {
                qualified: NS.Inner;
                qualifiedClass: NS.K;
                viaGen: Gen2<Headers>;
                viaId: Id<Headers>;
                partial: Gen2;
            }
        "#);
        let result = |n: &str| result(&ir, n);
        let read = |n: &str| owner(&imp(&ir, n).member.result);
        // The type parameter may be `X | null`, so the other empty is admitted too.
        assert_eq!(result("Gen.opt_t"), Some(WasmType::JS));
        assert_eq!(result("Gen.nullable_t"), Some(WasmType::JS));
        assert_eq!(result("Gen.plain_t"), Some(WasmType::JS));
        // Qualified alias resolves through the namespace scope.
        assert!(result("Uses.qualified").unwrap().admits_null());
        assert_eq!(read("Uses.qualified"), OwnerRef::global("Headers"));
        assert_eq!(result("Uses.qualified_class"), Some(WasmType::JS_NON_NULL));
        assert_eq!(read("Uses.qualified_class"), OwnerRef::global("NS.K"));
        // Generic alias keeps the head once the argument is substituted.
        assert!(result("Uses.via_gen").unwrap().admits_null());
        assert_eq!(read("Uses.via_gen"), OwnerRef::global("Headers"));
        assert_eq!(result("Uses.via_id"), Some(WasmType::JS_NON_NULL));
        assert_eq!(read("Uses.via_id"), OwnerRef::global("Headers"));
        // An alias parameter without an argument or default erases to `any`.
        assert_eq!(result("Uses.partial"), Some(WasmType::JS));
        assert_eq!(*head(&imp(&ir, "Uses.partial").member.result), H::Any);
    }

    #[test]
    fn members_bind_under_every_export_name() {
        let class = ir(r#"
            declare module "m:a" {
                class _Foo { constructor(); get(): string; }
                export { _Foo as Foo, _Foo as Bar };
            }
        "#);
        // The class binds under its Rust name, and under each public name.
        for (name, aliases) in [
            ("_Foo.new", ["Foo.new", "Bar.new"]),
            ("_Foo.get", ["Foo.get", "Bar.get"]),
        ] {
            assert_eq!(imp(&class, name).names[1..], aliases);
        }

        let ns = ir(r#"
            declare module "m:a" {
                namespace _N {
                    function nf(): void;
                    class C { constructor(); }
                }
                export { _N as N1, _N as N2 };
            }
        "#);
        for (name, alias) in [("N1.nf", "N2.nf"), ("N1.C.new", "N2.C.new")] {
            assert_eq!(imp(&ns, name).names, [name, alias]);
        }
    }

    #[test]
    fn module_binds_only_its_export_map() {
        let ir = ir(r#"
            declare module "m:a" {
                function open(): Hidden;
                function hidden(): void;
                const secret: number;
                class Hidden { static make(): Hidden; m(): void; }
                export { open };
            }
        "#);
        assert!(find(&ir, "open").is_some());
        assert!(find(&ir, "hidden").is_none());
        assert!(find(&ir, "secret").is_none());
        // An unexported class keeps only what binds through a value.
        assert!(find(&ir, "Hidden.m").is_some());
        assert!(find(&ir, "Hidden.make").is_none());
    }

    #[test]
    fn renamed_exports_bind_by_their_public_name() {
        let ir = ir(r#"
            declare module "m:a" {
                class _Foo { constructor(); }
                namespace _N { function f(): void; }
                export { _Foo as Foo, _N as N };
            }
        "#);
        let ctor = imp(&ir, "_Foo.new");
        assert_eq!(ctor.names[1..], ["Foo.new"]);
        assert_eq!(ctor.member.kind, MemberKind::Constructor);
        assert_eq!(ctor.member.js, "Foo");
        assert_eq!(ctor.member.result.wasm(), Some(WasmType::JS_NON_NULL));
        assert_eq!(owner(&ctor.member.result), declared(Some("m:a"), "_Foo"));
        assert!(find(&ir, "N.f").is_some());
    }

    #[test]
    fn heads_name_the_module_of_their_owner() {
        let ir = ir(r#"
            declare class Thing { g(): void; }
            declare function global(): Thing;
            declare module "m:a" {
                class Thing { m(): void; }
                function local(): Thing;
            }
        "#);
        let local = imp_in(&ir, "m:a", "local").member;
        let global = imp(&ir, "global").member;
        assert_eq!(owner(&local.result), declared(Some("m:a"), "Thing"));
        assert_eq!(owner(&global.result), OwnerRef::global("Thing"));
    }

    #[test]
    fn module_scalar_exports_keep_their_type() {
        let ir = ir(r#"
            declare module "m:a" { const n: number; let b: boolean; const l: bigint; }
        "#);
        for (name, ty) in [
            ("n", WasmType::F64),
            ("b", WasmType::I32),
            ("l", WasmType::I64),
        ] {
            let m = imp_in(&ir, "m:a", name).member;
            assert_eq!(m.result.wasm(), Some(ty), "{name}");
        }
    }

    #[test]
    fn null_and_undefined_bits_and_heads() {
        let ir = ir(r#"
            declare class Headers { constructor(); }
            type MaybeHeaders = Headers | null;
            interface Init {
                headers?: Headers;
                nullable: MaybeHeaders;
                both?: Headers | null;
            }
            declare function f(h: MaybeHeaders): Headers;
        "#);
        let m = |n: &str| imp(&ir, n).member;
        let headers = OwnerRef::global("Headers");
        assert_eq!(result(&ir, "Init.headers"), Some(UND));
        assert_eq!(result(&ir, "Init.nullable"), Some(NUL));
        assert_eq!(result(&ir, "Init.both"), Some(WasmType::JS));
        assert_eq!(param(&ir, "f", 0), WasmType::JS_NON_NULL);
        assert_eq!(param(&ir, "f_with_null", 0), NUL);
        assert_eq!(result(&ir, "f"), Some(WasmType::JS_NON_NULL));
        assert_eq!(owner(&m("Init.nullable").result), headers);
        assert_eq!(owner(&m("f").params[0].ty), headers);
        assert_eq!(head(&m("f_with_null").params[0].ty), &H::Empty);
        assert_eq!(owner(&m("f").result), headers);
        assert_eq!(owner(&m("Headers.new").result), headers);
    }

    #[test]
    fn string_positions_have_the_string_head() {
        let ir = ir(r#"
            type S = string;
            declare class Blob {}
            declare function a(x: string | null): S;
            declare function b(): "x" | "y";
            declare function c(): string | number;
            declare function d(): string | Blob;
        "#);
        let m = |n: &str| imp(&ir, n).member;
        assert_eq!(head(&m("a").params[0].ty), &H::String);
        assert_eq!(head(&m("a").result), &H::String);
        // A union is a kind; one of string literals has only string arms.
        let arms = |n: &str| {
            let kind = owner(&m(n).result);
            ir.kind(None, &kind.path)
                .unwrap_or_else(|| panic!("{n} reads no kind"))
                .arms
                .iter()
                .map(|a| a.ty.head.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            arms("b"),
            [
                H::Literal(Literal::String("x".into())),
                H::Literal(Literal::String("y".into()))
            ]
        );
        assert_eq!(arms("c"), [H::String, H::Number]);
        assert_eq!(arms("d")[0], H::String);
    }

    #[test]
    fn null_arms_bind_as_rust_does() {
        let ir = ir(r#"
            declare class Body { constructor(); }
            declare class Response {
                constructor(body?: Body | null, init?: string);
            }
            interface Init { body?: Body | null; limit?: number | null; empties?: null; }
            declare function key(x: string | number | null): void;
        "#);
        let ty = |n: &str, i: usize| param(&ir, n, i);
        assert_eq!(ty("Response.new_with_body", 0), WasmType::JS_NON_NULL);
        assert_eq!(ty("Response.new_with_null", 0), NUL);
        assert_eq!(ty("Response.new_with_null_and_init", 0), NUL);
        let null_only = &imp(&ir, "Response.new_with_null").member.params;
        assert_eq!(null_only.len(), 1);
        assert_eq!(head(&null_only[0].ty), &H::Empty);
        assert_eq!(ty("Init.set_body", 0), WasmType::JS_NON_NULL);
        assert_eq!(ty("Init.set_body_with_null", 0), NUL);
        assert_eq!(ty("Init.set_limit", 0), WasmType::F64);
        assert_eq!(ty("Init.set_limit_with_null", 0), NUL);
        assert_eq!(ty("key", 0), WasmType::JS_NON_NULL);
        assert_eq!(ty("key_with_f64", 0), WasmType::F64);
        assert_eq!(ty("key_with_null", 0), NUL);
        assert_eq!(ty("Init.set_empties_with_null", 0), NUL);
    }

    #[test]
    fn same_name_in_two_roots_is_two_imports() {
        let ir = ir(r#"
            declare function connect(address: string): void;
            declare module "cloudflare:sockets" {
                function connect(address: string): void;
            }
            declare module "node:net" {
                namespace constants { const O_RDONLY: Sock; }
                class Sock {}
            }
            declare module "node:fs" {
                namespace constants { const O_RDONLY: Sock; }
                class Sock {}
            }
        "#);
        for module in [BINDINGS_SPECIFIER, "cloudflare:sockets"] {
            imp_in(&ir, module, "connect");
        }
        for module in ["node:net", "node:fs"] {
            let f = imp_in(&ir, module, "constants.o_rdonly");
            assert_eq!(f.member.kind, MemberKind::StaticGet);
            assert_eq!(f.class.unwrap().path, "constants");
            assert_eq!(f.member.js, "O_RDONLY");
        }
    }

    fn param_names_of(params: &[ts_gen_abi::ir::Param]) -> Vec<&str> {
        params.iter().map(|p| p.name.as_str()).collect()
    }

    #[test]
    fn param_names_are_numbered_where_they_collide_once_sanitised() {
        let ir = ir(
            "declare function collideArgs(a: string, $a: number, fooBar: string, foo_bar: number): void;",
        );
        assert_eq!(
            param_names_of(&imp(&ir, "collide_args").member.params),
            ["a", "a2", "foo_bar", "foo_bar2"]
        );
    }

    #[test]
    fn receiver_name_is_reserved() {
        // The receiver crosses as `this`.
        let ir = ir("declare class Thing { method(_this: string, value: number): void; }");
        let method = imp(&ir, "Thing.method").member;
        assert_eq!(param_names_of(&method.params), ["this2", "value"]);
    }

    #[test]
    fn non_ascii_param_names_are_legal_wat_identifiers() {
        let ir = ir("declare function uni(café: string, cafè: number): void;");
        let uni = imp(&ir, "uni").member;
        assert_eq!(param_names_of(&uni.params), ["caf_", "caf_2"]);
    }

    #[test]
    fn handler_params_are_numbered_where_they_collide_once_sanitised() {
        let (module, mut gctx) = crate::parse_source(
            "interface Handler { handle(fooBar: string, foo_bar: number, café: string): void; }",
            None,
        )
        .unwrap();
        let provided = crate::provides::resolve("Handler.handle", &module, &mut gctx).unwrap();
        let ir = build_ir(&module, &gctx, &GenerateOptions::new(), &provided);
        assert_eq!(
            param_names_of(&ir.provides[0].params),
            ["foo_bar", "foo_bar2", "caf_"]
        );
        assert_eq!(
            ts_gen_abi::text::Printer::new(&ir).provide(&ir.provides[0]),
            "provide default.handle(foo_bar: String, foo_bar2: f64, caf_: String);"
        );
    }

    fn ir(src: &str) -> Ir {
        ir_with(src, false)
    }

    fn ir_with(src: &str, fallible: bool) -> Ir {
        let (module, gctx) = crate::parse_source(src, None).unwrap();
        let options = exports(&module, &gctx).fallible(fallible);
        build_ir(&module, &gctx, &options, &[])
    }

    fn member<'a>(ir: &'a Ir, class: &str, name: &str) -> &'a ts_gen_abi::ir::Member {
        ir.classes
            .iter()
            .filter(|c| c.name == class)
            .flat_map(|c| &c.members)
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no {class}.{name}"))
    }

    fn function<'a>(ir: &'a Ir, name: &str) -> &'a ts_gen_abi::ir::Member {
        ir.functions
            .iter()
            .map(|f| &f.member)
            .find(|m| m.name == name)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn names(members: &[ts_gen_abi::ir::Member]) -> Vec<&str> {
        members.iter().map(|m| m.name.as_str()).collect()
    }

    fn head(ty: &ts_gen_abi::ir::Type) -> &ts_gen_abi::ir::Head {
        match ty {
            ts_gen_abi::ir::Type::Js(js) => &js.head,
            other => panic!("not a JS type: {other:?}"),
        }
    }

    fn named(ty: &ts_gen_abi::ir::Type) -> (&str, Vec<&ts_gen_abi::ir::Head>) {
        match head(ty) {
            ts_gen_abi::ir::Head::Named(n) => (
                n.name.path.as_str(),
                n.args.iter().map(|a| &a.head).collect(),
            ),
            other => panic!("not named: {other:?}"),
        }
    }

    #[test]
    fn setter_unions_expand_per_arm() {
        use ts_gen_abi::ir::{Head as H, MemberKind, Type};
        let ir = ir(r#"
            declare class Headers { constructor(); }
            interface Init {
                headers?: Headers | Record<string, string>;
                body?: string | ArrayBuffer | null;
            }
        "#);
        let init = ir.class(None, "Init").unwrap();
        assert_eq!(
            names(&init.members),
            [
                "new",
                "headers",
                "set_headers",
                "set_headers_with_record",
                "body",
                "set_body",
                "set_body_with_array_buffer",
                "set_body_with_null",
            ]
        );
        let set = |n: &str| member(&ir, "Init", n);
        assert_eq!(set("set_headers").kind, MemberKind::Set);
        assert_eq!(set("set_headers").js, "headers");
        assert_eq!(set("set_headers").params[0].name, "value");
        assert_eq!(named(&set("set_headers").params[0].ty), ("Headers", vec![]));
        assert_eq!(
            named(&set("set_headers_with_record").params[0].ty),
            ("Record", vec![&H::String])
        );
        assert_eq!(head(&set("set_body").params[0].ty), &H::String);
        assert_eq!(head(&set("set_body_with_null").params[0].ty), &H::Empty);
        assert_eq!(set("set_body").result, Type::Void);
        // The read takes the whole union.
        assert!(matches!(set("body").result, Type::Js(ref j) if j.undefined));
    }

    #[test]
    fn kinds_carry_rust_names_arms_and_aliases() {
        use ts_gen_abi::ir::{Head as H, Literal};
        let ir = ir(r#"
            declare class Headers { constructor(); }
            type Mode = "fast" | "safe";
            declare enum Level { Low = 1, High = 2, Max = 2 }
            declare function setMode(m: Mode): void;
            declare function level(): Level;
            declare function pick(): string | Headers;
            declare module "m" {
                export class Day { constructor(); }
                export interface DayFields { day: number; }
                export type DayLike = Day | DayFields | string;
                export function init(d: DayLike): DayLike;
            }
        "#);
        let kind = |n: &str| {
            ir.kinds
                .iter()
                .find(|k| k.name == n)
                .unwrap_or_else(|| panic!("no kind {n}: {:?}", ir.kinds))
        };
        let mode = kind("Mode");
        let arm_names = |k: &ts_gen_abi::ir::Kind| -> Vec<String> {
            k.arms.iter().map(|a| a.name.clone()).collect()
        };
        assert_eq!(arm_names(mode), ["Fast", "Safe"]);
        assert_eq!(
            mode.arms[0].ty.head,
            H::Literal(Literal::String("fast".into()))
        );
        assert!(!mode.fallback);
        let level = kind("Level");
        assert_eq!(arm_names(level), ["Low", "High"]);
        assert_eq!(level.arms[1].aliases, ["Max"]);
        assert_eq!(level.arms[1].ty.head, H::Literal(Literal::Int(2)));
        let day = kind("DayLike");
        assert_eq!(day.module.as_deref(), Some("m"));
        assert_eq!(arm_names(day), ["Day", "String", "DayFields"]);
        assert!(
            day.fallback,
            "the dictionary arm takes what the others don't"
        );
        // An argument takes the alias whole, as a kind, and a read is it.
        let init = function(&ir, "init");
        assert_eq!(named(&init.params[0].ty), ("DayLike", vec![]));
        assert_eq!(named(&init.result), ("DayLike", vec![]));
        assert_eq!(
            named(&function(&ir, "set_mode").params[0].ty),
            ("Mode", vec![])
        );
        // A union read is the enum Rust synthesises for it.
        let (pick, _) = named(&function(&ir, "pick").result);
        assert!(kind(pick).arms.len() == 2, "{pick}");
    }

    #[test]
    fn throws_follows_rust_catch() {
        let ir = ir(r#"
            declare class TypeError { constructor(); }
            declare class RangeError { constructor(); }
            declare class Thing {
                constructor();
                plain(): number;
                /** @throws {TypeError} */
                typed(): number;
                /** @throws {TypeError | RangeError} */
                either(): number;
                pick<T>(xs: T[]): T;
                /** @throws {never} */
                safe(): number;
                later(): Promise<string>;
            }
        "#);
        let m = |n: &str| member(&ir, "Thing", n);
        assert!(m("new").throws, "constructors always catch");
        assert!(!m("plain").throws && !m("plain").nothrow && m("plain").errors.is_empty());
        let error = |path: &str| {
            ts_gen_abi::ir::Head::Named(ts_gen_abi::ir::Named {
                name: OwnerRef::global(path),
                args: Vec::new(),
            })
        };
        let heads = |n: &str| -> Vec<ts_gen_abi::ir::Head> {
            m(n).errors.iter().map(|e| e.head.clone()).collect()
        };
        assert!(!m("typed").throws);
        assert_eq!(heads("typed"), [error("TypeError")]);
        assert_eq!(heads("either"), [error("TypeError"), error("RangeError")]);
        assert_eq!(m("pick").type_params, ["T"]);
        assert!(m("safe").nothrow);
        // A promise binding catches what the call throws, not the rejection.
        assert!(!m("later").throws);
        let fallible = ir_with(
            r#"declare module "m" {
                export class Thing { plain(): number; later(): Promise<string>; }
            }"#,
            true,
        );
        assert!(member(&fallible, "Thing", "plain").throws);
        assert!(member(&fallible, "Thing", "later").throws);
    }

    #[test]
    fn promises_and_containers_keep_their_type_arguments() {
        use ts_gen_abi::ir::Head as H;
        let ir = ir(r#"
            declare class Blob {}
            declare function text(): Promise<string>;
            declare function blobs(): Promise<Blob[]>;
            declare function done(): Promise<void>;
            declare function map(m: ReadonlyMap<string, number>): ArrayLike<Blob>;
            declare function spread<A extends unknown[]>(...args: A): void;
        "#);
        assert_eq!(
            named(&function(&ir, "text").result),
            ("Promise", vec![&H::String])
        );
        let (promise, args) = named(&function(&ir, "blobs").result);
        assert_eq!(promise, "Promise");
        assert!(matches!(args[0], H::Named(n) if n.name.path == "Array"));
        assert_eq!(
            named(&function(&ir, "done").result),
            ("Promise", vec![&H::Empty])
        );
        let map = function(&ir, "map");
        assert_eq!(
            named(&map.params[0].ty),
            ("Map", vec![&H::String, &H::Number])
        );
        assert_eq!(map.params[0].ty.wasm(), Some(WasmType::JS_NON_NULL));
        assert_eq!(named(&map.result).0, "Array");
        let spread = function(&ir, "spread");
        assert!(spread.variadic);
        assert_eq!(named(&spread.params[0].ty).0, "Array");
    }

    #[test]
    fn literal_arms_share_their_cohort_binding() {
        use ts_gen_abi::ir::{Head as H, Literal};
        let ir = ir(r#"
            interface KV {
                mode(kind: "fast" | "safe"): boolean;
                get(name: "Set-Cookie"): string;
            }
        "#);
        let kv = ir.class(None, "KV").unwrap();
        assert_eq!(names(&kv.members), ["mode_fast", "mode_safe", "get"]);
        for (name, lit) in [("mode_fast", "fast"), ("mode_safe", "safe")] {
            let m = member(&ir, "KV", name);
            assert_eq!(m.js, "mode");
            assert_eq!(m.cohort.as_deref(), Some("__mode"));
            assert_eq!(
                head(&m.params[0].ty),
                &H::Literal(Literal::String(lit.into()))
            );
        }
        // A lone literal is no cohort.
        assert_eq!(member(&ir, "KV", "get").cohort, None);
    }

    #[test]
    fn classes_bind_under_their_rust_names() {
        let ir = ir(r#"
            declare module "m:a" {
                class _Foo { constructor(); }
                export { _Foo as Foo };
            }
            declare module "lh" {
                interface H { x: number; }
                export function u(h: H): void;
                export {};
            }
            declare module "lh" {
                export interface H { y?: number; }
            }
        "#);
        let foo = ir.class(Some("m:a"), "_Foo").unwrap();
        assert_eq!(foo.path, "Foo");
        assert_eq!(foo.aliases, ["Foo"]);
        assert_eq!(member(&ir, "_Foo", "new").js, "Foo");
        // A block's local `H` beside the module's exported one.
        let local = ir.class(Some("lh"), "H2").unwrap();
        assert_eq!(
            (local.path.as_str(), names(&local.members)),
            ("H", vec!["new", "x", "set_x"])
        );
        let exported = ir.class(Some("lh"), "H").unwrap();
        assert_eq!(names(&exported.members), ["new", "y", "set_y"]);
        assert!(!member(&ir, "H2", "x").optional);
        assert!(member(&ir, "H", "y").optional);
        let u = ir.functions.iter().find(|f| f.member.name == "u").unwrap();
        assert_eq!(named(&u.member.params[0].ty).0, "H2");
    }

    #[test]
    fn member_names_escape_as_rust_does() {
        let ir = ir(r#"
            declare class Pair { readonly 0: string; static readonly 1: string; }
            declare var self: Pair;
        "#);
        let pair = ir.class(None, "Pair").unwrap();
        assert_eq!(names(&pair.members), ["_0", "_1"]);
        assert_eq!(pair.members[0].js, "0");
        assert_eq!(function(&ir, "self_").js, "self");
    }
}
