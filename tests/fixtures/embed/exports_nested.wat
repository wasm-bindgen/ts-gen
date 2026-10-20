;; Provides the exports in exports_nested.exports.d.ts. Nested paths are
;; wasm export names joined with `.`; the wrapper rebuilds the objects.
(module
  (import "./bindings.wasm" "Request.url" (func $Request.url (param externref) (result externref)))
  (import "./bindings.wasm" "Response.new_with_body" (func $Response.new (param externref) (result externref)))
  (import "wasm:js/string-constants" "ok" (global $ok externref))

  (func (export "version") (result f64) (f64.const 3))
  (func (export "api.math.add") (param $a f64) (param $b f64) (result f64)
    (f64.add (local.get $a) (local.get $b)))
  (func (export "api.describe") (param $req externref) (result externref)
    (call $Request.url (local.get $req)))
  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (call $Response.new (global.get $ok)))
)
