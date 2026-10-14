;; An anyref constructor result next to an externref dictionary `new`: both
;; bindings reach globalThis, whose import type must not depend on either.
(module
  (import "./bindings.wasm" "Headers.new" (func $Headers.new (result anyref)))
  (import "./bindings.wasm" "Headers.set" (func $Headers.set (param anyref externref externref)))
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new (result externref)))
  (import "./bindings.wasm" "ResponseInit.set_headers" (func $ResponseInit.set_headers (param externref anyref)))
  (import "./bindings.wasm" "Response.new_with_body_and_init"
    (func $Response.new (param externref externref) (result externref)))

  (import "wasm:js/string-constants" "content-type" (global $content_type externref))
  (import "wasm:js/string-constants" "text/plain" (global $text_plain externref))
  (import "wasm:js/string-constants" "ok" (global $ok externref))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $headers anyref) (local $init externref)
    (local.set $headers (call $Headers.new))
    (call $Headers.set (local.get $headers) (global.get $content_type) (global.get $text_plain))
    (local.set $init (call $ResponseInit.new))
    (call $ResponseInit.set_headers (local.get $init) (local.get $headers))
    (call $Response.new (global.get $ok) (local.get $init)))
)
