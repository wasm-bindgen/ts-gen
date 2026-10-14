;; JSPI mode: imports await, so wrapper should use WebAssembly.promising.
(module
  (import "./bindings.wasm" "Request.url" (func $Request.url (param externref) (result externref)))
  (import "./bindings.wasm" "Request.method" (func $Request.method (param externref) (result externref)))
  (import "./bindings.wasm" "Request.headers" (func $Request.headers (param externref) (result externref)))
  (import "./bindings.wasm" "Request.text" (func $Request.text (param externref) (result externref)))
  (import "./bindings.wasm" "Headers.get" (func $Headers.get (param externref externref) (result externref)))
  (import "./bindings.wasm" "Headers.new" (func $Headers.new (result externref)))
  (import "./bindings.wasm" "Headers.set" (func $Headers.set (param externref externref externref)))
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new (result externref)))
  (import "./bindings.wasm" "ResponseInit.set_status" (func $ResponseInit.set_status (param externref f64)))
  (import "./bindings.wasm" "ResponseInit.set_headers" (func $ResponseInit.set_headers (param externref externref)))
  (import "./bindings.wasm" "Response.new_with_body_and_init"
    (func $Response.new (param externref externref) (result externref)))
  (import "./builtins.js" "await" (func $await (param externref) (result externref)))

  (import "wasm:js-string" "equals" (func $str.eq (param externref externref) (result i32)))
  (import "wasm:js-string" "concat" (func $str.concat (param externref externref) (result (ref extern))))
  (import "wasm:js/string-constants" "GET" (global $GET externref))
  (import "wasm:js/string-constants" "Hello from Wasm!" (global $hello externref))
  (import "wasm:js/string-constants" "content-type" (global $content_type externref))
  (import "wasm:js/string-constants" "text/plain" (global $text_plain externref))
  (import "wasm:js/string-constants" "x-name" (global $x_name externref))
  (import "wasm:js/string-constants" ": " (global $colon externref))

  (func $respond (param $body externref) (param $status f64) (result externref)
    (local $init externref) (local $headers externref)
    (local.set $headers (call $Headers.new))
    (call $Headers.set (local.get $headers) (global.get $content_type) (global.get $text_plain))
    (local.set $init (call $ResponseInit.new))
    (call $ResponseInit.set_status (local.get $init) (local.get $status))
    (call $ResponseInit.set_headers (local.get $init) (local.get $headers))
    (call $Response.new (local.get $body) (local.get $init)))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $body externref)
    (if (call $str.eq (call $Request.method (local.get $req)) (global.get $GET))
      (then (return (call $respond (global.get $hello) (f64.const 200)))))
    ;; POST: echo "<x-name>: <body>", awaiting the body through JSPI
    (local.set $body (call $await (call $Request.text (local.get $req))))
    (call $respond
      (call $str.concat
        (call $str.concat
          (call $Headers.get (call $Request.headers (local.get $req)) (global.get $x_name))
          (global.get $colon))
        (local.get $body))
      (f64.const 200))))
