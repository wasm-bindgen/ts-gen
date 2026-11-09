(module
  (type $str (array (mut i8)))
  (import "tw:tag" "KvError" (tag $kv (param i32 externref)))
  (import "tw:js/global" "Kv#get(Str)" (func $get (param externref externref) (result externref)))
  (import "tw:js/global" "Kv#raw(Str)" (func $raw (param externref externref) (result externref)))
  (import "tw:js/global" "Kv#name(str)" (func $name (param externref (ref $str)) (result (ref $str))))
  (import "tw:js/global" "Kv#nameRaw(str)" (func $nameRaw (param externref (ref $str)) (result (ref $str))))
  (export "tag" (tag $kv))

  ;; (0, value), or the caught tag's (arm, message).
  (func (export "get") (param externref externref) (result i32 externref)
    (block $h (result i32 externref)
      (try_table (catch $kv $h)
        (return (i32.const 0) (call $get (local.get 0) (local.get 1))))
      (unreachable)))

  ;; Catches everything and rethrows the same exception.
  (func (export "raw") (param externref externref) (result externref)
    (throw_ref
      (block $h (result exnref)
        (try_table (catch_all_ref $h)
          (return (call $raw (local.get 0) (local.get 1))))
        (unreachable))))

  (func (export "relay") (param externref externref) (result externref)
    (call $get (local.get 0) (local.get 1)))

  ;; (0, result length, null), or (arm, 0, message).
  (func (export "name") (param externref i32) (result i32 i32 externref)
    (local $m externref)
    (block $h (result i32 externref)
      (try_table (catch $kv $h)
        (return
          (i32.const 0)
          (array.len (call $name (local.get 0) (array.new $str (i32.const 97) (local.get 1))))
          (ref.null extern)))
      (unreachable))
    (local.set $m)
    (i32.const 0)
    (local.get $m))

  (func (export "nameRaw") (param externref i32) (result i32)
    (array.len (call $nameRaw (local.get 0) (array.new $str (i32.const 97) (local.get 1))))))
