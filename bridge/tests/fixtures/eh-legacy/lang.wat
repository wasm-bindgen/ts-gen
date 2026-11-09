(module
  (type $str (array (mut i8)))
  (import "tw:tag" "KvError" (tag $kv (param i32 externref)))
  (import "tw:js/global" "Kv#get(Str)" (func $get (param externref externref) (result externref)))
  (import "tw:js/global" "Kv#raw(Str)" (func $raw (param externref externref) (result externref)))
  (import "tw:js/global" "Kv#name(str)" (func $name (param externref (ref $str)) (result (ref $str))))
  (import "tw:js/global" "Kv#nameRaw(str)" (func $nameRaw (param externref (ref $str)) (result (ref $str))))
  (export "tag" (tag $kv))

  (func (export "get") (param externref externref) (result i32 externref)
    try (result i32 externref)
      i32.const 0
      (call $get (local.get 0) (local.get 1))
    catch $kv
    end)

  (func (export "raw") (param externref externref) (result externref)
    try (result externref)
      (call $raw (local.get 0) (local.get 1))
    catch_all
      rethrow 0
    end)

  (func (export "relay") (param externref externref) (result externref)
    (call $get (local.get 0) (local.get 1)))

  (func (export "name") (param externref i32) (result i32 i32 externref)
    (local $m externref)
    try (result i32 i32 externref)
      i32.const 0
      (array.len (call $name (local.get 0) (array.new $str (i32.const 97) (local.get 1))))
      ref.null extern
    catch $kv
      local.set $m
      i32.const 0
      local.get $m
    end)

  (func (export "nameRaw") (param externref i32) (result i32)
    (array.len (call $nameRaw (local.get 0) (array.new $str (i32.const 97) (local.get 1))))))
