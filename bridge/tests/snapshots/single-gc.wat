(module
  (type $d0 (array (mut i16)))
  (import "tw:glue" "g10" (func $g10 (param externref) (result externref)))
  (import "tw:glue" "g11" (func $g11 (param externref externref i32 f64)))
  (import "tw:glue" "spill_fault" (func $fault (param i32)))
  (import "wasm:js-string" "length" (func $js_length (param externref) (result i32)))
  (import "wasm:js-string" "fromCharCodeArray" (func $js_from (param (ref null $d0)) (param i32 i32) (result (ref extern))))
  (import "wasm:js-string" "intoCharCodeArray" (func $js_into (param externref) (param (ref null $d0)) (param i32) (result i32)))
  (type $ft0 (func (param i32) (result (ref $d0))))
  (memory $m (export "memory") 1)
  (table $st (export "table") 0 externref)
  (global (export "hdr") i32 (i32.const 512))
  (data (i32.const 512) "\48\02\00\00")
  (func $sp (result i32) (i32.load (i32.const 512)))
  (func $tsp (result i32) (i32.load offset=4 (i32.const 512)))
  (func $set_sp (param $v i32) (i32.store (i32.const 512) (local.get $v)))
  (func $set_tsp (param $v i32) (i32.store offset=4 (i32.const 512) (local.get $v)))
  (func $salloc (param $size i32) (param $align i32) (result i32)
    (local $p i32) (local $end i32) (local $have i32)
    (local.set $p (i32.and (i32.add (call $sp) (i32.sub (local.get $align) (i32.const 1))) (i32.sub (i32.const 0) (local.get $align))))
    (local.set $end (i32.add (local.get $p) (local.get $size)))
    (local.set $have (i32.shl (memory.size) (i32.const 16)))
    (if (i32.gt_u (local.get $end) (local.get $have))
      (then (if (i32.eq (memory.grow (i32.shr_u (i32.add (i32.sub (local.get $end) (local.get $have)) (i32.const 65535)) (i32.const 16))) (i32.const -1)) (then unreachable))))
    (call $set_sp (local.get $end))
    (local.get $p))
  (func $stalloc (param $n i32) (result i32)
    (local $b i32) (local $end i32)
    (local.set $b (call $tsp))
    (local.set $end (i32.add (local.get $b) (local.get $n)))
    (if (i32.gt_u (local.get $end) (table.size $st))
      (then (if (i32.eq (table.grow $st (ref.null extern) (i32.sub (local.get $end) (table.size $st))) (i32.const -1)) (then unreachable))))
    (call $set_tsp (local.get $end))
    (local.get $b))
  (func $restore (param $sp i32) (param $tsp i32)
    (table.fill $st (local.get $tsp) (ref.null extern) (i32.sub (call $tsp) (local.get $tsp)))
    (call $set_tsp (local.get $tsp))
    (call $set_sp (local.get $sp)))
  (table $tw (export "tw:table") 132 externref)
  (global (export "tw:table/stack_base") i32 (i32.const 1))
  (global (export "tw:table/const_base") i32 (i32.const 128))
  (global (export "tw:table/heap_base") i32 (i32.const 132))
  (global $rh (mut i32) (i32.const 128))
  (global $rt (mut i32) (i32.const 128))
  (global $nh (mut i32) (i32.const 0))
  (global $nt (mut i32) (i32.const 0))
  (func $settle
    (if (i32.eq (global.get $rh) (global.get $rt))
      (then (global.set $rh (i32.const 128)) (global.set $rt (i32.const 128))))
    (if (i32.eq (global.get $nh) (global.get $nt))
      (then (global.set $nh (i32.const 0)) (global.set $nt (i32.const 0)))))
  (func $nslot (result i32)
    (if (i32.ge_u (global.get $nt) (i32.const 64)) (then (call $fault (i32.const 1)) unreachable))
    (global.set $nt (i32.add (global.get $nt) (i32.const 1)))
    (i32.shl (i32.sub (global.get $nt) (i32.const 1)) (i32.const 3)))
  (func $nnext (result i32)
    (if (i32.ge_u (global.get $nh) (global.get $nt)) (then (call $fault (i32.const 0)) unreachable))
    (global.set $nh (i32.add (global.get $nh) (i32.const 1)))
    (i32.shl (i32.sub (global.get $nh) (i32.const 1)) (i32.const 3)))
  (func $push_externref (export "push_externref") (param $v externref)
    (if (i32.le_u (global.get $rt) (i32.const 1)) (then (call $fault (i32.const 1)) unreachable))
    (global.set $rt (i32.sub (global.get $rt) (i32.const 1)))
    (table.set $tw (global.get $rt) (local.get $v)))
  (func $pop_externref (export "pop_externref") (result externref)
    (local $v externref)
    (if (i32.le_u (global.get $rh) (global.get $rt)) (then (call $fault (i32.const 0)) unreachable))
    (global.set $rh (i32.sub (global.get $rh) (i32.const 1)))
    (local.set $v (table.get $tw (global.get $rh)))
    (table.set $tw (global.get $rh) (ref.null extern))
    (call $settle)
    (local.get $v))
  (func $spill_reset (export "spill_reset")
    (table.fill $tw (global.get $rt) (ref.null extern) (i32.sub (i32.const 128) (global.get $rt)))
    (global.set $rh (i32.const 128))
    (global.set $rt (i32.const 128))
    (global.set $nh (i32.const 0))
    (global.set $nt (i32.const 0)))
  (func $spill_empty (export "spill_empty") (result i32)
    (i32.and (i32.eq (global.get $rh) (global.get $rt)) (i32.eq (global.get $nh) (global.get $nt))))
  (func $spill_check
    (if (i32.eqz (call $spill_empty)) (then (call $fault (i32.const 2)) unreachable)))
  (func $push_i32 (export "push_i32") (param $v i32) (i32.store (call $nslot) (local.get $v)))
  (func $pop_i32 (export "pop_i32") (result i32) (local $v i32) (local.set $v (i32.load (call $nnext))) (call $settle) (local.get $v))
  (func $push_i64 (export "push_i64") (param $v i64) (i64.store (call $nslot) (local.get $v)))
  (func $pop_i64 (export "pop_i64") (result i64) (local $v i64) (local.set $v (i64.load (call $nnext))) (call $settle) (local.get $v))
  (func $push_f32 (export "push_f32") (param $v f32) (f32.store (call $nslot) (local.get $v)))
  (func $pop_f32 (export "pop_f32") (result f32) (local $v f32) (local.set $v (f32.load (call $nnext))) (call $settle) (local.get $v))
  (func $push_f64 (export "push_f64") (param $v f64) (f64.store (call $nslot) (local.get $v)))
  (func $pop_f64 (export "pop_f64") (result f64) (local $v f64) (local.set $v (f64.load (call $nnext))) (call $settle) (local.get $v))
  (table $entries (export "entries") 1 funcref)
  (func (export "a16get") (param $a (ref null $d0)) (param $i i32) (result i32) (array.get_u $d0 (local.get $a) (local.get $i)))
  (func (export "a16set") (param $a (ref null $d0)) (param $i i32) (param $v i32) (array.set $d0 (local.get $a) (local.get $i) (local.get $v)))
  (func $h2 (param $p0 externref) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (if (ref.is_null (local.get $p0)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (call $js_length (local.get $p0))))
    (drop (call $js_into (local.get $p0) (local.get $l1) (i32.const 0)))
    local.get $l1
  )
  (func $h1 (param $p0 externref) (param $p1 i32) (param $p2 f64) (result (ref null $d0) i32 f64)
    (local $l1 (ref null $d0))
    (local $l2 (ref $d0))
    local.get $p0
    call $h2
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    local.get $l2
    local.get $p1
    local.get $p2
  )
  (func $f10 (export "f10") (param $p0 externref) (result (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 i32)
    (local $l103 f64)
    (local $l104 (ref null $d0))
    (local $l105 i32)
    (local $l106 f64)
    (local $l107 (ref $d0))
    (local $l108 (ref null $d0))
    (local $l109 i32)
    (local $l110 f64)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g10
      local.set $l101
      (local.set $l102 (i32.load offset=520 (i32.const 0)))
      (local.set $l103 (f64.load offset=528 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      call $h1
      local.set $l106
      local.set $l105
      local.set $l104
      (local.set $l107 (ref.as_non_null (local.get $l104)))
      (local.set $l108 (local.get $l107))
      (local.set $l109 (local.get $l105))
      (local.set $l110 (local.get $l106))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (call $spill_check)
    (call $push_i32 (local.get $l109))
    (call $push_f64 (local.get $l110))
    (ref.as_non_null (local.get $l108))
  )
  (func $h4 (param $p0 (ref null $d0)) (result externref)
    (if (result externref) (ref.is_null (local.get $p0)) (then (ref.null extern)) (else (call $js_from (local.get $p0) (i32.const 0) (array.len (local.get $p0)))))
  )
  (func $h3 (param $p0 (ref null $d0)) (param $p1 i32) (param $p2 f64) (result externref i32 f64)
    (local $l1 externref)
    local.get $p0
    call $h4
    local.set $l1
    local.get $l1
    local.get $p1
    local.get $p2
  )
  (func $f11 (export "f11") (param $p0 externref) (param $p1 (ref $d0)) (param $p2 i32) (param $p3 f64)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 i32)
    (local $l103 f64)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      local.get $p2
      local.get $p3
      call $h3
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      call $g11
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $e0 (export "e0") (param $p0 i32) (result externref i32 f64)
    (local $l1 (ref $d0))
    (local $l2 i32)
    (local $l3 f64)
    (local $l4 externref)
    (local $l5 i32)
    (local $l6 f64)
    local.get $p0
    (call_indirect $entries (type $ft0) (i32.const 0))
    (call $pop_i32)
    (call $pop_f64)
    (call $spill_check)
    local.set $l3
    local.set $l2
    local.set $l1
    local.get $l1
    local.get $l2
    local.get $l3
    call $h3
    local.set $l6
    local.set $l5
    local.set $l4
    local.get $l4
    local.get $l5
    local.get $l6
  )
)
