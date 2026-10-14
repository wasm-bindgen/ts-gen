;; Non-nullable ABI positions imported as (ref extern): the bindings wrap
;; those results in ref.as_non_null. Headers.get is `string | null` and
;; stays externref.
(module
  (import "./bindings.wasm" "Request.url" (func $Request.url (param (ref extern)) (result (ref extern))))
  (import "./bindings.wasm" "Request.headers" (func $Request.headers (param (ref extern)) (result (ref extern))))
  (import "./bindings.wasm" "Headers.get" (func $Headers.get (param (ref extern) (ref extern)) (result externref)))
  ;; JS produces nothing here: the module's own argument may be narrowed.
  (import "./bindings.wasm" "JsValue.is_undefined" (func $is_undefined (param (ref extern)) (result i32)))
  (import "./bindings.wasm" "Response.new_with_body" (func $Response.new (param externref) (result (ref extern))))
  ;; An imported string constant is a non-null externref global in V8.
  (import "wasm:js/string-constants" "x-name" (global $x_name (ref extern)))

  (func (export "fetch") (param $req (ref extern)) (param $env externref) (param $ctx externref) (result (ref extern))
    (local $name externref)
    (local.set $name (call $Headers.get (call $Request.headers (local.get $req)) (global.get $x_name)))
    (if (result (ref extern)) (i32.or (ref.is_null (local.get $name)) (call $is_undefined (call $Request.url (local.get $req))))
      (then (call $Response.new (call $Request.url (local.get $req))))
      (else (call $Response.new (local.get $name)))))
)
