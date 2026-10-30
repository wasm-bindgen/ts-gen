;; Hand-written per ABI.md, to develop the generator against before the
;; emitter writes annotations. Covers every form in the grammar.
(module
  (@abi.version 1)
  (type $anyrefs (array (mut anyref)))

  (import "./bindings.wasm" "JsValue.is_undefined" (func $JsValue.is_undefined (@abi.is-undefined)
    (param $x externref) (result i32)))
  (import "./bindings.wasm" "JsValue.undefined" (func $JsValue.undefined (@abi.undefined)
    (result externref)))
  (import "./bindings.wasm" "Number.value" (func $Number.value (@abi.number)
    (param $x externref) (result f64)))

  (@abi.class "Headers" (doc "HTTP headers.\nA second line with a */ in it."))
  (import "./bindings.wasm" "Headers.new" (func $Headers.new
    (@abi.new "Headers") (@abi.owner "Headers") (@abi.ts "constructor()")
    (result externref (@abi.ref "Headers"))))
  (import "./bindings.wasm" "Headers.new_with_init" (func $Headers.new_with_init
    (@abi.new "Headers") (@abi.owner "Headers") (@abi.ts "constructor(init: HeadersInit)")
    (param $init externref)
    (result externref (@abi.ref "Headers"))))
  (import "./bindings.wasm" "Headers.get" (func $Headers.get
    (@abi.call "get") (@abi.owner "Headers")
    (@abi.ts "get(name: string): string | null")
    (@abi.doc "Returns the first value of the header.")
    (param $this externref (@abi.ref "Headers"))
    (param $name externref (@abi.ref string))
    (result externref (@abi.ref string null))))
  (import "./bindings.wasm" "Headers.has" (func $Headers.has
    (@abi.call "has") (@abi.owner "Headers")
    (param $this externref (@abi.ref "Headers"))
    (param $name externref (@abi.ref string))
    (result i32)))
  (import "./bindings.wasm" "Headers.set" (func $Headers.set
    (@abi.call "set") (@abi.owner "Headers")
    (param $this externref (@abi.ref "Headers"))
    (param $name externref (@abi.ref string))
    (param $value externref (@abi.ref string))))
  (import "./bindings.wasm" "Headers.delete" (func $Headers.delete
    (@abi.call "delete") (@abi.owner "Headers")
    (param $this externref (@abi.ref "Headers"))
    (param $name externref (@abi.ref string))))

  (@abi.interface "Body")
  (import "./bindings.wasm" "Body.text" (func $Body.text
    (@abi.call "text") (@abi.owner "Body") (@abi.ts "text(): Promise<string>")
    (param $this externref (@abi.ref "Body"))
    (result externref)))
  (import "./bindings.wasm" "Body.body_used" (func $Body.body_used
    (@abi.get "bodyUsed") (@abi.owner "Body")
    (param $this externref (@abi.ref "Body"))
    (result i32)))

  (@abi.class "Request" (extends "Body") (doc "An HTTP request."))
  (import "./bindings.wasm" "Request.new" (func $Request.new
    (@abi.new "Request") (@abi.owner "Request")
    (param $input externref)
    (result externref (@abi.ref "Request"))))
  (import "./bindings.wasm" "Request.new_with_init" (func $Request.new_with_init
    (@abi.new "Request") (@abi.owner "Request")
    (param $input externref) (param $init externref (@abi.ref "RequestInit"))
    (result externref (@abi.ref "Request"))))
  (import "./bindings.wasm" "Request.method" (func $Request.method
    (@abi.get "method") (@abi.owner "Request")
    (param $this externref (@abi.ref "Request"))
    (result externref (@abi.ref string))))
  (import "./bindings.wasm" "Request.url" (func $Request.url
    (@abi.get "url") (@abi.owner "Request")
    (param $this externref (@abi.ref "Request"))
    (result externref (@abi.ref string))))
  (import "./bindings.wasm" "Request.headers" (func $Request.headers
    (@abi.get "headers") (@abi.owner "Request")
    (param $this externref (@abi.ref "Request"))
    (result externref (@abi.ref "Headers"))))
  (import "./bindings.wasm" "Request.body" (func $Request.body
    (@abi.get "body") (@abi.owner "Request")
    (param $this externref (@abi.ref "Request"))
    (result externref (@abi.ref null))))

  (@abi.dictionary "RequestInit")
  (import "./bindings.wasm" "RequestInit.new" (func $RequestInit.new
    (@abi.object) (@abi.owner "RequestInit")
    (result externref (@abi.ref "RequestInit"))))
  (import "./bindings.wasm" "RequestInit.method" (func $RequestInit.method
    (@abi.get "method") (@abi.owner "RequestInit")
    (param $this externref (@abi.ref "RequestInit"))
    (result externref (@abi.ref string undefined))))
  (import "./bindings.wasm" "RequestInit.set_method" (func $RequestInit.set_method
    (@abi.set "method") (@abi.owner "RequestInit")
    (param $this externref (@abi.ref "RequestInit"))
    (param $value externref (@abi.ref string))))
  (import "./bindings.wasm" "RequestInit.set_body" (func $RequestInit.set_body
    (@abi.set "body") (@abi.owner "RequestInit")
    (param $this externref (@abi.ref "RequestInit"))
    (param $value externref (@abi.ref null))))

  (@abi.class "Response")
  (import "./bindings.wasm" "Response.new" (func $Response.new
    (@abi.new "Response") (@abi.owner "Response")
    (result externref (@abi.ref "Response"))))
  (import "./bindings.wasm" "Response.new_with_body" (func $Response.new_with_body
    (@abi.new "Response") (@abi.owner "Response")
    (param $body externref (@abi.ref null))
    (result externref (@abi.ref "Response"))))
  (import "./bindings.wasm" "Response.new_with_body_and_init" (func $Response.new_with_body_and_init
    (@abi.new "Response") (@abi.owner "Response")
    (param $body externref (@abi.ref null undefined))
    (param $init externref (@abi.ref "ResponseInit"))
    (result externref (@abi.ref "Response"))))
  (import "./bindings.wasm" "Response.redirect" (func $Response.redirect
    (@abi.call "redirect" (on "Response")) (@abi.owner "Response")
    (param $url externref (@abi.ref string))
    (result externref (@abi.ref "Response"))))
  (import "./bindings.wasm" "Response.redirect_with_status" (func $Response.redirect_with_status
    (@abi.call "redirect" (on "Response")) (@abi.owner "Response")
    (param $url externref (@abi.ref string)) (param $status f64)
    (result externref (@abi.ref "Response"))))
  (import "./bindings.wasm" "Response.json" (func $Response.json
    (@abi.call "json" (on "Response")) (@abi.owner "Response")
    (param $data externref (@abi.ref null undefined))
    (result externref (@abi.ref "Response"))))
  (import "./bindings.wasm" "Response.json_2" (func $Response.json_2
    (@abi.call "json") (@abi.owner "Response")
    (param $this externref (@abi.ref "Response"))
    (result externref)))
  (import "./bindings.wasm" "Response.status" (func $Response.status
    (@abi.get "status") (@abi.owner "Response")
    (param $this externref (@abi.ref "Response"))
    (result f64)))
  (import "./bindings.wasm" "Response.overload_a" (func $Response.overload_a
    (@abi.call "pick") (@abi.owner "Response")
    (param $this externref (@abi.ref "Response")) (param $x externref)))
  (import "./bindings.wasm" "Response.overload_b" (func $Response.overload_b
    (@abi.call "pick") (@abi.owner "Response")
    (param $this externref (@abi.ref "Response")) (param $x externref (@abi.ref null))))

  (@abi.dictionary "ResponseInit")
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new
    (@abi.object) (@abi.owner "ResponseInit")
    (result externref (@abi.ref "ResponseInit"))))
  (import "./bindings.wasm" "ResponseInit.status" (func $ResponseInit.status
    (@abi.get "status") (@abi.owner "ResponseInit")
    (param $this externref (@abi.ref "ResponseInit"))
    (result externref (@abi.ref number undefined))))
  (import "./bindings.wasm" "ResponseInit.set_status" (func $ResponseInit.set_status
    (@abi.set "status") (@abi.owner "ResponseInit")
    (param $this externref (@abi.ref "ResponseInit")) (param $value f64)))

  (@abi.interface "Env")
  (import "./bindings.wasm" "Env.greeting" (func $Env.greeting
    (@abi.get "GREETING") (@abi.owner "Env")
    (param $this externref (@abi.ref "Env"))
    (result externref (@abi.ref string))))

  (@abi.class "ExecutionContext")
  (import "./bindings.wasm" "ExecutionContext.wait_until" (func $ExecutionContext.wait_until
    (@abi.call "waitUntil") (@abi.owner "ExecutionContext")
    (param $this externref (@abi.ref "ExecutionContext")) (param $promise externref)))

  (@abi.interface "Flags")
  (import "./bindings.wasm" "Flags.set_verbose" (func $Flags.set_verbose
    (@abi.set "verbose") (@abi.owner "Flags")
    (param $this externref (@abi.ref "Flags")) (param $value i32)))

  (@abi.namespace "WebAssembly")
  (@abi.class "WebAssembly.Module")
  (import "./bindings.wasm" "WebAssembly.Module.exports" (func $WebAssembly.Module.exports
    (@abi.call "exports" (on "WebAssembly" "Module")) (@abi.owner "WebAssembly.Module")
    (param $module externref (@abi.ref "WebAssembly.Module"))
    (result externref)))
  (import "./bindings.wasm" "WebAssembly.validate" (func $WebAssembly.validate
    (@abi.call "validate" (on "WebAssembly")) (@abi.owner "WebAssembly")
    (param $bytes externref) (result i32)))

  (import "./bindings.wasm" "fetch" (func $fetch
    (@abi.call "fetch" (on)) (@abi.ts "fetch(input: RequestInfo): Promise<Response>")
    (param $input externref) (result externref)))
  (import "./bindings.wasm" "fetch_with_init" (func $fetch_with_init
    (@abi.call "fetch" (on))
    (param $input externref) (param $init externref (@abi.ref "RequestInit")) (result externref)))
  (@abi.interface "Console")
  (import "./bindings.wasm" "Console.log" (func $Console.log
    (@abi.call "log") (@abi.owner "Console")
    (param $this externref (@abi.ref "Console")) (param $args (ref null $anyrefs))))
  (import "./bindings.wasm" "console" (func $console
    (@abi.get "console" (on))
    (result externref (@abi.ref "Console"))))

  (@abi.class "Conn" (module "db") (alias "Connection"))
  (import "db" "Conn.get" (func $db:Conn.get (@abi.call "get") (@abi.owner "Conn")
    (param $this externref (@abi.ref "Conn" (module "db"))) (result externref (@abi.ref string))))
  (import "db" "Connection.get" (func $db:Connection.get (@abi.alias "Conn.get")
    (param $this externref) (result externref)))

  (import "./builtins.js" "undefined" (global $undefined externref))
  (import "./builtins.js" "await" (func $await (param externref) (result externref)))

  (func (export "fetch")
    (@abi.provide "default" "fetch") (@abi.promise)
    (@abi.ts "fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response>")
    (param $request externref (@abi.ref "Request"))
    (param $env externref (@abi.ref "Env"))
    (param $ctx externref (@abi.ref "ExecutionContext"))
    (result externref (@abi.ref "Response"))
    unreachable)
)
