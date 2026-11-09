(module
  (type $d0 (array (mut i16)))
  (type $d1 (array (mut i64)))
  (import "tw:glue" "g0" (func $g0 (param externref externref) (result externref)))
  (import "tw:glue" "g1" (func $g1 (param externref externref externref) (result externref)))
  (import "tw:glue" "g3" (func $g3 (param externref externref) (result i32)))
  (import "tw:glue" "g4" (func $g4 (param externref) (result externref)))
  (import "tw:glue" "g5" (func $g5 (param externref) (result externref)))
  (import "tw:glue" "g6" (func $g6 (param externref) (result externref)))
  (import "tw:glue" "g11" (func $g11 (param externref) (result externref)))
  (import "tw:glue" "r13" (func $r13 (param externref) (result i32)))
  (import "tw:glue" "r14" (func $r14 (param externref) (result i32)))
  (import "tw:glue" "g15" (func $g15 (param i32 i32) (result externref)))
  (import "tw:glue" "g16" (func $g16 (param externref) (result i32)))
  (import "wasm:js-string" "length" (func $js_length (param externref) (result i32)))
  (import "wasm:js-string" "fromCharCodeArray" (func $js_from (param (ref null $d0)) (param i32 i32) (result (ref extern))))
  (import "wasm:js-string" "intoCharCodeArray" (func $js_into (param externref) (param (ref null $d0)) (param i32) (result i32)))
  (type $ft0 (func (param anyref i32) (result i32)))
  (type $ft1 (func (param externref) (result anyref)))
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
  (table $entries (export "entries") 2 funcref)
  (func (export "a16get") (param $a (ref null $d0)) (param $i i32) (result i32) (array.get_u $d0 (local.get $a) (local.get $i)))
  (func (export "a16set") (param $a (ref null $d0)) (param $i i32) (param $v i32) (array.set $d0 (local.get $a) (local.get $i) (local.get $v)))
  (func $h1 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $h2 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $f0 (export "f0") (param $p0 externref) (param $p1 anyref) (result anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 externref)
    (local $l103 anyref)
    (local $l104 anyref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h1
      local.set $l101
      local.get $p0
      local.get $l101
      call $g0
      local.set $l102
      local.get $l102
      call $h2
      local.set $l103
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
  )
  (func $f1 (export "f1") (param $p0 externref) (param $p1 anyref) (param $p2 anyref) (result externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 externref)
    (local $l103 externref)
    (local $l104 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h1
      local.set $l101
      local.get $p2
      call $h1
      local.set $l102
      local.get $p0
      local.get $l101
      local.get $l102
      call $g1
      local.set $l103
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
  )
  (func $h3 (param $p0 (ref null $d0)) (result externref)
    (if (result externref) (ref.is_null (local.get $p0)) (then (ref.null extern)) (else (call $js_from (local.get $p0) (i32.const 0) (array.len (local.get $p0)))))
  )
  (func $f3 (export "f3") (param $p0 externref) (param $p1 (ref $d0)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h3
      local.set $l101
      local.get $p0
      local.get $l101
      call $g3
      local.set $l102
      (local.set $l103 (i32.load offset=8 (i32.const 0)))
      (local.set $l104 (local.get $l102))
      (local.set $l105 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
    local.get $l105
  )
  (func $h4 (param $p0 externref) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (if (ref.is_null (local.get $p0)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (call $js_length (local.get $p0))))
    (drop (call $js_into (local.get $p0) (local.get $l1) (i32.const 0)))
    local.get $l1
  )
  (func $f4 (export "f4") (param $p0 externref) (result (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 (ref null $d0))
    (local $l103 (ref $d0))
    (local $l104 (ref null $d0))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g4
      local.set $l101
      local.get $l101
      call $h4
      local.set $l102
      (local.set $l103 (ref.as_non_null (local.get $l102)))
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l104))
  )
  (func $h5 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $f5 (export "f5") (param $p0 anyref) (result externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 externref)
    (local $l103 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h5
      local.set $l101
      local.get $l101
      call $g5
      local.set $l102
      (local.set $l103 (local.get $l102))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l103
  )
  (func $h6 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $f6 (export "f6") (param $p0 externref) (result anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 anyref)
    (local $l103 anyref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g6
      local.set $l101
      local.get $l101
      call $h6
      local.set $l102
      (local.set $l103 (local.get $l102))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l103
  )
  (func $h7 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $f11 (export "f11") (param $p0 anyref) (result externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l102 externref)
    (local $l103 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h7
      local.set $l101
      local.get $l101
      call $g11
      local.set $l102
      (local.set $l103 (local.get $l102))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l103
  )
  (func $f13 (export "f13") (param $p0 externref) (result i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r13 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
  )
  (func $f14 (export "f14") (param $p0 externref) (result i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r14 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
  )
  (func $h8 (param $p0 (ref null $d1)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 8)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i64.store (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 8))) (array.get $d1 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f15 (export "f15") (param $p0 (ref $d1)) (result externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 externref)
    (local $l104 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h8
      local.set $l102
      local.set $l101
      local.get $l101
      local.get $l102
      call $g15
      local.set $l103
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
  )
  (func $h9 (param $p0 i32) (param $p1 i32) (result (ref null $d1))
    (local $l1 (ref null $d1))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d1))))
    (local.set $l1 (array.new_default $d1 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d1 (local.get $l1) (local.get $l2) (i64.load (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f16 (export "f16") (param $p0 externref) (result (ref $d1))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d1))
    (local $l104 (ref $d1))
    (local $l105 (ref null $d1))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g16
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h9
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h10 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $e0 (export "e0") (param $p0 externref) (param $p1 i32) (result i32)
    (local $l1 anyref)
    (local $l2 i32)
    local.get $p0
    call $h10
    local.set $l1
    local.get $l1
    local.get $p1
    (call_indirect $entries (type $ft0) (i32.const 0))
    local.set $l2
    local.get $l2
  )
  (func $e1 (export "e1") (param $p0 externref) (result externref)
    (local $l1 anyref)
    (local $l2 externref)
    local.get $p0
    (call_indirect $entries (type $ft1) (i32.const 1))
    local.set $l1
    local.get $l1
    call $h5
    local.set $l2
    local.get $l2
  )
)
