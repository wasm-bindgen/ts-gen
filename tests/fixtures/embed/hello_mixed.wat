;; Mixed ref types: anyref params, externref results, for
;; both bindings imports and the `await` builtin.
(module
  (import "./bindings.wasm" "Request.url" (func $Request.url (param anyref) (result externref)))
  (import "./bindings.wasm" "Request.method" (func $Request.method (param anyref) (result externref)))
  (import "./bindings.wasm" "Request.text" (func $Request.text (param anyref) (result externref)))
  (import "./bindings.wasm" "Response.new_with_body_and_init"
    (func $Response.new (param externref externref) (result externref)))
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new (result externref)))
  (import "./builtins.js" "await" (func $await (param anyref) (result externref)))

  (import "wasm:js-string" "equals" (func $str.eq (param externref externref) (result i32)))
  (import "wasm:js/string-constants" "GET" (global $GET externref))

  (func (export "fetch") (param $req anyref) (param $env anyref) (param $ctx anyref) (result externref)
    ;; GET: echo the URL; otherwise echo the awaited body
    (if (result externref) (call $str.eq (call $Request.method (local.get $req)) (global.get $GET))
      (then (call $Response.new (call $Request.url (local.get $req)) (call $ResponseInit.new)))
      (else (call $Response.new
        (call $await (any.convert_extern (call $Request.text (local.get $req))))
        (call $ResponseInit.new)))))
)
