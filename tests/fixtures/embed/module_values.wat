;; Scalar module exports, imported in their declared types, and a module
;; function.
(module
  (import "embed:values" "count" (func $count (result f64)))
  (import "embed:values" "enabled" (func $enabled (result i32)))
  (import "embed:values" "disabled" (func $disabled (result i32)))
  (import "embed:values" "big" (func $big (result i64)))
  (import "embed:values" "receiver" (func $receiver (result externref)))

  (func (export "count") (result f64) (call $count))
  (func (export "enabled") (result i32) (call $enabled))
  (func (export "disabled") (result i32) (call $disabled))
  (func (export "big") (result i64) (call $big))
  (func (export "receiver") (result externref) (call $receiver))
)
