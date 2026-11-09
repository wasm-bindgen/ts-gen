;; A language misusing the spill: each misuse is a TypeError, not a trap.
(module
  (import "tw:js/global" "P#two()" (func $two (param externref) (result i32)))
  (import "tw:ret" "i32" (func $pop (result i32)))
  (import "tw:ret" "push:i32" (func $push (param i32)))
  (func (export "under") (result i32) (call $pop))
  (func (export "unread") (param $p externref)
    (drop (call $two (local.get $p)))
    (drop (call $two (local.get $p))))
  (func (export "read") (param $p externref) (result i32 i32)
    (call $two (local.get $p))
    (call $pop))
  (func (export "over") (local $i i32)
    (loop $l
      (call $push (local.get $i))
      (local.set $i (i32.add (local.get $i) (i32.const 1)))
      (br $l))))
