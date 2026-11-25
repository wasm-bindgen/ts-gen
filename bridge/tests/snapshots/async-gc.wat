(module
  (type $d0 (array (mut externref)))
  (type $d1 (array (mut i16)))
  (type $d2 (array (mut i8)))
  (import "tw:glue" "g1" (func $g1 (param externref) (result i32)))
  (import "tw:glue" "g3" (func $g3 (param externref) (result externref)))
  (import "tw:glue" "g5" (func $g5 (param i32 i32) (result i32)))
  (import "tw:glue" "g6" (func $g6 (param i32 i32 i32)))
  (import "tw:glue" "g8" (func $g8 (param externref externref)))
  (import "wasm:js-string" "length" (func $js_length (param externref) (result i32)))
  (import "wasm:js-string" "fromCharCodeArray" (func $js_from (param (ref null $d1)) (param i32 i32) (result (ref extern))))
  (import "wasm:js-string" "intoCharCodeArray" (func $js_into (param externref) (param (ref null $d1)) (param i32) (result i32)))
  (memory $m (export "memory") 1)
  (table $st (export "table") 0 externref)
  (global (export "hdr") i32 (i32.const 0))
  (data (i32.const 0) "\48\00\00\00")
  (func $sp (result i32) (i32.load (i32.const 0)))
  (func $tsp (result i32) (i32.load offset=4 (i32.const 0)))
  (func $set_sp (param $v i32) (i32.store (i32.const 0) (local.get $v)))
  (func $set_tsp (param $v i32) (i32.store offset=4 (i32.const 0) (local.get $v)))
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
  (func (export "a16get") (param $a (ref null $d1)) (param $i i32) (result i32) (array.get_u $d1 (local.get $a) (local.get $i)))
  (func (export "a16set") (param $a (ref null $d1)) (param $i i32) (param $v i32) (array.set $d1 (local.get $a) (local.get $i) (local.get $v)))
  (func $h1 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 externref)
    (local $l6 externref)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (if (i32.eqz (local.get $p1)) (then (return (array.new_fixed $d0 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p1)))
    (local.set $l3 (i32.add (i32.const 0) (i32.mul (local.get $l2) (i32.const 0))))
    (local.set $l4 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))
    (local.set $l5 (table.get $st (i32.add (local.get $l4) (i32.const 0))))
    local.get $l5
    local.set $l6
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d0 (local.get $l6) (local.get $p1)))) (else (array.set $d0 (local.get $l1) (local.get $l2) (local.get $l6))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f1 (export "f1") (param $p0 externref) (result (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d0))
    (local $l104 (ref $d0))
    (local $l105 (ref null $d0))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g1
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h1
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h3 (param $p0 externref) (result (ref null $d1))
    (local $l1 (ref null $d1))
    (if (ref.is_null (local.get $p0)) (then (return (ref.null $d1))))
    (local.set $l1 (array.new_default $d1 (call $js_length (local.get $p0))))
    (drop (call $js_into (local.get $p0) (local.get $l1) (i32.const 0)))
    local.get $l1
  )
  (func $h2 (param $p0 i32) (param $p1 externref) (result i32 (ref null $d1))
    (local $l1 (ref null $d1))
    (local $l2 (ref $d1))
    local.get $p1
    call $h3
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    local.get $p0
    local.get $l2
  )
  (func $f3 (export "f3") (param $p0 externref) (result i32 (ref $d1))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 externref)
    (local $l103 i32)
    (local $l104 (ref null $d1))
    (local $l105 (ref $d1))
    (local $l106 i32)
    (local $l107 (ref null $d1))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g3
      local.set $l102
      (local.set $l101 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h2
      local.set $l104
      local.set $l103
      (local.set $l105 (ref.as_non_null (local.get $l104)))
      (local.set $l106 (local.get $l103))
      (local.set $l107 (local.get $l105))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l106
    (ref.as_non_null (local.get $l107))
  )
  (func $h4 (param $p0 (ref null $d0)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 externref)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l3 (call $stalloc (i32.mul (local.get $l1) (i32.const 1))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 0))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 1))))
    (array.get $d0 (local.get $p0) (local.get $l4))
    local.set $l7
    (table.set $st (i32.add (local.get $l6) (i32.const 0)) (local.get $l7))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l3
    local.get $l1
  )
  (func $h5 (param $p0 i32) (param $p1 i32) (result (ref null $d2))
    (local $l1 (ref null $d2))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d2))))
    (local.set $l1 (array.new_default $d2 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d2 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f5 (export "f5") (param $p0 (ref $d0)) (result (ref $d2))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 (ref null $d2))
    (local $l106 (ref $d2))
    (local $l107 (ref null $d2))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h4
      local.set $l102
      local.set $l101
      local.get $l101
      local.get $l102
      call $g5
      local.set $l103
      (local.set $l104 (i32.load offset=8 (i32.const 0)))
      local.get $l103
      local.get $l104
      call $h5
      local.set $l105
      (local.set $l106 (ref.as_non_null (local.get $l105)))
      (local.set $l107 (local.get $l106))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l107))
  )
  (func $f6 (export "f6") (param $p0 (ref $d0)) (param $p1 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h4
      local.set $l102
      local.set $l101
      local.get $l101
      local.get $l102
      local.get $p1
      call $g6
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h6 (param $p0 (ref null $d1)) (result externref)
    (if (result externref) (ref.is_null (local.get $p0)) (then (ref.null extern)) (else (call $js_from (local.get $p0) (i32.const 0) (array.len (local.get $p0)))))
  )
  (func $f8 (export "f8") (param $p0 externref) (param $p1 (ref $d1))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h6
      local.set $l101
      local.get $p0
      local.get $l101
      call $g8
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
)
