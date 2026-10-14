;; Boolean writes: a dictionary field setter and a call argument. The
;; bindings must hand JS real booleans, not the Numbers 1/0.
(module
  (import "./bindings.wasm" "Flags.new" (func $Flags.new (result externref)))
  (import "./bindings.wasm" "Flags.set_verbose" (func $Flags.set_verbose (param externref i32)))
  (import "./bindings.wasm" "describeFlags" (func $describeFlags (param externref i32) (result externref)))
  (import "./bindings.wasm" "Response.new_with_body" (func $Response.new (param externref) (result externref)))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $flags externref)
    (local.set $flags (call $Flags.new))
    (call $Flags.set_verbose (local.get $flags) (i32.const 1))
    (call $Response.new (call $describeFlags (local.get $flags) (i32.const 0))))
)
