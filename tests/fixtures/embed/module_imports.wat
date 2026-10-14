;; Declarations from `declare module "cloudflare:sockets"`: the bindings
;; import the module's exports as externref globals, so nothing here needs
;; to know the specifier.
(module
  (import "./bindings.wasm" "connect" (func $connect (param externref) (result externref)))
  (import "./bindings.wasm" "Socket.protocol" (func $Socket.protocol (result externref)))
  (import "./bindings.wasm" "Socket.start_tls" (func $Socket.start_tls (param externref) (result externref)))
  (import "./bindings.wasm" "Socket.closed" (func $Socket.closed (param externref) (result externref)))
  (import "./bindings.wasm" "Response.new_with_body" (func $Response.new (param externref) (result externref)))
  (import "wasm:js-string" "concat" (func $str.concat (param externref externref) (result (ref extern))))
  (import "wasm:js/string-constants" "example.com:443" (global $address externref))
  (import "wasm:js/string-constants" ": " (global $colon externref))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $socket externref)
    (local.set $socket (call $Socket.start_tls (call $connect (global.get $address))))
    (drop (call $Socket.closed (local.get $socket)))
    (call $Response.new (call $str.concat (call $Socket.protocol) (global.get $colon))))
)
