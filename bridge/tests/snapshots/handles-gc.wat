(module
  (type $d0 (array (mut anyref)))
  (import "tw:glue" "g0" (func $g0 (param externref externref)))
  (import "tw:glue" "g1" (func $g1 (param externref externref)))
  (import "tw:glue" "g2" (func $g2 (param externref externref)))
  (import "tw:glue" "g3" (func $g3 (param externref)))
  (import "tw:glue" "g4" (func $g4 (param externref)))
  (import "tw:glue" "g6" (func $g6 (param externref) (result externref)))
  (import "tw:glue" "g7" (func $g7 (param externref) (result externref)))
  (import "tw:glue" "g8" (func $g8 (param externref) (result externref)))
  (import "tw:glue" "g9" (func $g9 (param externref) (result externref)))
  (import "tw:glue" "g10" (func $g10 (param externref) (result externref)))
  (type $ft0 (func (param anyref i32)))
  (type $ft1 (func (param anyref externref) (result externref)))
  (type $ft2 (func (param externref i32) (result anyref)))
  (type $ft3 (func (param externref anyref i32) (result i32)))
  (type $ft4 (func (param externref anyref externref) (result externref)))
  (type $ft5 (func (param externref anyref anyref) (result i32)))
  (type $ft6 (func (param externref anyref)))
  (type $ft7 (func (result anyref)))
  (type $ft8 (func (param anyref) (result anyref)))
  (type $ft9 (func (param (ref $d0)) (result i32)))
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
  (table $entries (export "entries") 10 funcref)
  (func $h1 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $f0 (export "f0") (param $p0 externref) (param $p1 anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
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
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h2 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $f1 (export "f1") (param $p0 externref) (param $p1 anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h2
      local.set $l101
      local.get $p0
      local.get $l101
      call $g1
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h3 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $f2 (export "f2") (param $p0 externref) (param $p1 anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
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
      call $g2
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $f3 (export "f3") (param $p0 anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h1
      local.set $l101
      local.get $l101
      call $g3
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $f4 (export "f4") (param $p0 anyref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $h3
      local.set $l101
      local.get $l101
      call $g4
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $f6 (export "f6") (param $p0 anyref) (result externref)
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
      call $h3
      local.set $l101
      local.get $l101
      call $g6
      local.set $l102
      (local.set $l103 (local.get $l102))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l103
  )
  (func $h4 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $f7 (export "f7") (param $p0 externref) (result anyref)
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
      call $g7
      local.set $l101
      local.get $l101
      call $h4
      local.set $l102
      (local.set $l103 (local.get $l102))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l103
  )
  (func $h5 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $f8 (export "f8") (param $p0 externref) (result anyref)
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
      call $g8
      local.set $l101
      local.get $l101
      call $h5
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
  (func $f9 (export "f9") (param $p0 externref) (result anyref)
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
      call $g9
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
  (func $h7 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $f10 (export "f10") (param $p0 externref) (result anyref)
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
      call $g10
      local.set $l101
      local.get $l101
      call $h7
      local.set $l102
      (local.set $l103 (local.get $l102))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l103
  )
  (func $h8 (param $p0 externref) (result anyref)
    (any.convert_extern (local.get $p0))
  )
  (func $e0 (export "e0") (param $p0 externref) (param $p1 i32)
    (local $l1 anyref)
    local.get $p0
    call $h8
    local.set $l1
    local.get $l1
    local.get $p1
    (call_indirect $entries (type $ft0) (i32.const 0))
  )
  (func $e1 (export "e1") (param $p0 externref) (param $p1 externref) (result externref)
    (local $l1 anyref)
    (local $l2 externref)
    local.get $p0
    call $h5
    local.set $l1
    local.get $l1
    local.get $p1
    (call_indirect $entries (type $ft1) (i32.const 1))
    local.set $l2
    local.get $l2
  )
  (func $e2 (export "e2") (param $p0 externref) (param $p1 i32) (result externref)
    (local $l1 anyref)
    (local $l2 externref)
    local.get $p0
    local.get $p1
    (call_indirect $entries (type $ft2) (i32.const 2))
    local.set $l1
    local.get $l1
    call $h3
    local.set $l2
    local.get $l2
  )
  (func $e3 (export "e3") (param $p0 externref) (param $p1 externref) (param $p2 i32) (result i32)
    (local $l1 anyref)
    (local $l2 i32)
    local.get $p1
    call $h4
    local.set $l1
    local.get $p0
    local.get $l1
    local.get $p2
    (call_indirect $entries (type $ft3) (i32.const 3))
    local.set $l2
    local.get $l2
  )
  (func $e4 (export "e4") (param $p0 externref) (param $p1 externref) (param $p2 externref) (result externref)
    (local $l1 anyref)
    (local $l2 externref)
    local.get $p1
    call $h4
    local.set $l1
    local.get $p0
    local.get $l1
    local.get $p2
    (call_indirect $entries (type $ft4) (i32.const 4))
    local.set $l2
    local.get $l2
  )
  (func $e5 (export "e5") (param $p0 externref) (param $p1 externref) (param $p2 externref) (result i32)
    (local $l1 anyref)
    (local $l2 anyref)
    (local $l3 i32)
    local.get $p1
    call $h4
    local.set $l1
    local.get $p2
    call $h6
    local.set $l2
    local.get $p0
    local.get $l1
    local.get $l2
    (call_indirect $entries (type $ft5) (i32.const 5))
    local.set $l3
    local.get $l3
  )
  (func $e6 (export "e6") (param $p0 externref) (param $p1 externref)
    (local $l1 anyref)
    local.get $p1
    call $h4
    local.set $l1
    local.get $p0
    local.get $l1
    (call_indirect $entries (type $ft6) (i32.const 6))
  )
  (func $e7 (export "e7") (result externref)
    (local $l1 anyref)
    (local $l2 externref)
    (call_indirect $entries (type $ft7) (i32.const 7))
    local.set $l1
    local.get $l1
    call $h3
    local.set $l2
    local.get $l2
  )
  (func $h9 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $e8 (export "e8") (param $p0 externref) (result externref)
    (local $l1 anyref)
    (local $l2 anyref)
    (local $l3 externref)
    local.get $p0
    call $h6
    local.set $l1
    local.get $l1
    (call_indirect $entries (type $ft8) (i32.const 8))
    local.set $l2
    local.get $l2
    call $h9
    local.set $l3
    local.get $l3
  )
  (func $h10 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 externref)
    (local $l6 anyref)
    (local $l7 anyref)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (if (i32.eqz (local.get $p1)) (then (return (array.new_fixed $d0 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p1)))
    (local.set $l3 (i32.add (i32.const 0) (i32.mul (local.get $l2) (i32.const 0))))
    (local.set $l4 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))
    (local.set $l5 (table.get $st (i32.add (local.get $l4) (i32.const 0))))
    local.get $l5
    call $h4
    local.set $l6
    local.get $l6
    local.set $l7
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d0 (local.get $l7) (local.get $p1)))) (else (array.set $d0 (local.get $l1) (local.get $l2) (local.get $l7))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $e9 (export "e9") (param $p0 i32) (param $p1 i32) (result i32)
    (local $l1 (ref null $d0))
    (local $l2 (ref $d0))
    (local $l3 i32)
    local.get $p0
    local.get $p1
    call $h10
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    local.get $l2
    (call_indirect $entries (type $ft9) (i32.const 9))
    local.set $l3
    local.get $l3
  )
)
