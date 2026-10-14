;; The sys surface every language gets: `undefined`, `JsValue.is_undefined`
;; and `Number.value`. Echoes `status + 1` when the header is present.
(module
  (import "./bindings.wasm" "JsValue.is_undefined" (func $is_undefined (param externref) (result i32)))
  (import "./bindings.wasm" "Number.value" (func $number (param externref) (result f64)))
  (import "./bindings.wasm" "Request.headers" (func $Request.headers (param externref) (result externref)))
  (import "./bindings.wasm" "Headers.get" (func $Headers.get (param externref externref) (result externref)))
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new (result externref)))
  (import "./bindings.wasm" "ResponseInit.set_status" (func $set_status (param externref f64)))
  (import "./bindings.wasm" "Response.new_with_body_and_init"
    (func $Response.new (param externref externref) (result externref)))
  (import "./builtins.js" "undefined" (global $undefined externref))
  (import "wasm:js/string-constants" "x-status" (global $x_status externref))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $status externref) (local $init externref)
    (local.set $status (call $Headers.get (call $Request.headers (local.get $req)) (global.get $x_status)))
    (local.set $init (call $ResponseInit.new))
    ;; Headers.get returns null when absent; check both empties.
    (if (i32.or (ref.is_null (local.get $status)) (call $is_undefined (local.get $status)))
      (then (call $set_status (local.get $init) (f64.const 204)))
      (else (call $set_status (local.get $init) (f64.add (call $number (local.get $status)) (f64.const 1)))))
    (call $Response.new (global.get $undefined) (local.get $init)))
)
