;; A worker that proxies to the global `fetch`: the module imports
;; `./bindings.wasm` `fetch` and exports its own `fetch` handler, and the
;; merge must link the one and publish the other.
(module
  (import "./bindings.wasm" "fetch" (func $fetch (param externref) (result externref)))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (call $fetch (local.get $req))))
