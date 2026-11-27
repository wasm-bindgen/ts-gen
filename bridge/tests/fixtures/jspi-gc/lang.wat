;; A GC language under JSPI, with the spill and `throws = js`: `echo`
;; enters through the bridge module's GC entry, `race` passes a GC list,
;; and `fail` reports its error once resumed.
(module
  (type $str (array (mut i16)))
  (type $list (array (mut externref)))
  (import "tw:js/global" "Api#later()" (func $later (param externref) (result externref)))
  (import "tw:future" "suspend" (func $suspend (param externref) (result i32)))
  (import "tw:future" "suspend_list" (func $race (param (ref $list)) (result i32)))
  (import "tw:future" "get<u32>" (func $get (param externref) (result i32)))
  (import "tw:ret" "i32" (func $pop_i32 (result i32)))
  (import "tw:js/global" "Api#mark()" (func $mark (param externref)))
  (import "tw:future" "new" (func $new (result externref)))
  (import "tw:ret" "externref" (func $pop (result externref)))
  (import "tw:future" "resolve<u32>" (func $resolve (param externref i32)))
  (import "tw:err" "set:err" (func $fail (param i32 externref)))

  (func $ready (param $v i32) (result externref)
    (local $fut externref) (local $res externref)
    (local.set $fut (call $new))
    (local.set $res (call $pop))
    (call $resolve (local.get $res) (local.get $v))
    (local.get $fut))

  ;; The value of a fulfilled future, its option's value spilled.
  (func $value (param $f externref) (result i32)
    (if (i32.eqz (call $get (local.get $f))) (then unreachable))
    (call $pop_i32))

  (func $await (param $f externref) (result i32)
    (if (i32.ne (call $suspend (local.get $f)) (i32.const 1)) (then unreachable))
    (call $value (local.get $f)))

  (func (export "app.run(Api)") (param $api externref) (result externref)
    (local $a externref) (local $b externref)
    (local.set $a (call $later (local.get $api)))
    (local.set $b (call $later (local.get $api)))
    (call $ready (i32.add (call $await (local.get $a)) (call $await (local.get $b)))))

  (func (export "app.echo(Api,str)") (param $api externref) (param $s (ref $str)) (result externref)
    (local $v i32)
    (local.set $v (call $await (call $later (local.get $api))))
    (call $ready (i32.add (local.get $v) (array.get_u $str (local.get $s) (i32.const 0)))))

  (func (export "app.race(Api)") (param $api externref) (result externref)
    (local $l (ref null $list)) (local $i i32) (local $v i32)
    (local.set $l (array.new_fixed $list 3
      (call $later (local.get $api))
      (call $later (local.get $api))
      (call $later (local.get $api))))
    (local.set $i (call $race (ref.as_non_null (local.get $l))))
    (local.set $v (call $value (array.get $list (local.get $l) (local.get $i))))
    (call $ready (i32.add (i32.mul (local.get $i) (i32.const 100)) (local.get $v))))

  ;; Fails with `Error(msg)` once `api.later()` settles.
  (func (export "app.fail(Api,Str)") (param $api externref) (param $msg externref) (result externref)
    (drop (call $await (call $later (local.get $api))))
    (call $fail (i32.const 0) (local.get $msg))
    (ref.null extern))

  ;; A suspension with no stack to suspend throws, here into a handler,
  ;; and the language goes on with the bridge's scratch where it was.
  (func (export "app.stuck(Api)") (param $api externref) (result i32)
    (block $caught
      (try_table (catch_all $caught)
        (drop (call $suspend (call $later (local.get $api))))
        unreachable))
    (call $mark (local.get $api))
    (i32.const 7))
)
