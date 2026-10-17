;; Scalar module exports, imported in their declared types.
(module
  (import "embed:values" "count" (func $count (result f64)))
  (import "embed:values" "enabled" (func $enabled (result i32)))
  (import "embed:values" "disabled" (func $disabled (result i32)))
  (import "embed:values" "big" (func $big (result i64)))

  (func (export "count") (result f64) (call $count))
  (func (export "enabled") (result f64) (f64.convert_i32_u (call $enabled)))
  (func (export "disabled") (result f64) (f64.convert_i32_u (call $disabled)))
  (func (export "big") (result i64) (call $big))
)
