;; Non-JSPI mode: no await import, handler returns Response directly.
(module
  (import "./bindings.wasm" "Request.url" (func $Request.url (param externref) (result externref)))
  (import "./bindings.wasm" "Headers.new" (func $Headers.new (result externref)))
  (import "./bindings.wasm" "Headers.set" (func $Headers.set (param externref externref externref)))
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new (result externref)))
  (import "./bindings.wasm" "ResponseInit.set_status" (func $ResponseInit.set_status (param externref f64)))
  (import "./bindings.wasm" "ResponseInit.set_headers" (func $ResponseInit.set_headers (param externref externref)))
  (import "./bindings.wasm" "Response.new_with_body_and_init"
    (func $Response.new (param externref externref) (result externref)))

  (import "wasm:js/string-constants" "Hello from Wasm!" (global $hello externref))
  (import "wasm:js/string-constants" "content-type" (global $content_type externref))
  (import "wasm:js/string-constants" "text/plain" (global $text_plain externref))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $init externref) (local $headers externref)
    (local.set $headers (call $Headers.new))
    (call $Headers.set (local.get $headers) (global.get $content_type) (global.get $text_plain))
    (local.set $init (call $ResponseInit.new))
    (call $ResponseInit.set_status (local.get $init) (f64.const 200))
    (call $ResponseInit.set_headers (local.get $init) (local.get $headers))
    (call $Response.new (global.get $hello) (local.get $init))))
