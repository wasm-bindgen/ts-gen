use super::lower;

fn ir(src: &str) -> String {
    let (module, gctx) = crate::parse_source_with(src, None, crate::parse::Options::IR).unwrap();
    let lowered = lower(&module, &gctx);
    let text = ts_gen_abi::text::print(&lowered.ir);
    let back = ts_gen_abi::text::parse(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
    assert_eq!(back, lowered.ir);
    if let Err(errors) = ts_gen_abi::validate::validate(&lowered.ir) {
        let errors: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
        panic!("{}\n{text}", errors.join("\n"));
    }
    for d in &lowered.diagnostics {
        println!("// {d}");
    }
    text
}

#[test]
fn smoke() {
    println!(
        "{}",
        ir(r#"
declare function foo(a: string): void;
declare namespace foo { function baz(): number; }
declare class Response extends Body { constructor(body?: string | null); readonly status: number; static json(x: any): Response; }
declare class Body { text(): Promise<string>; }
interface RequestInit { method?: string; signal?: AbortSignal | null; }
declare var REQ: RequestInit;
declare module "node:buffer" { export class Buffer { concat(b: Buffer): Buffer; } }
type Method = "GET" | "POST";
declare function go(m: Method, x: number | string): void;
interface KV {
  get(key: string): Promise<string | null>;
  get(key: string, type: "text"): Promise<string | null>;
  get(key: string, type: "json"): Promise<any>;
  get(key: string[], type: "text"): Promise<Map<string, string | null>>;
}
declare function f(a?: string, b?: number | null, c?: boolean): void;
"#)
    );
}

#[test]
fn unexported_module_class_is_an_interface() {
    let text = ir(r#"
declare module "m" {
  class Hidden { constructor(); static make(): Hidden; get secret(): string; }
  export class Shown { hidden(): Hidden; }
  export {};
}
"#);
    assert!(text.contains("interface Hidden {"), "{text}");
    assert!(!text.contains("make"), "{text}");
    assert!(text.contains("export { Shown };"), "{text}");
}
