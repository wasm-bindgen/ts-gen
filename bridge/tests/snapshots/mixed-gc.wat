(module
  (type $d0 (array (mut i8)))
  (import "tw:glue" "r0" (func $r0 (param externref) (result i32)))
  (import "tw:glue" "r2" (func $r2 (param externref) (result i32)))
  (import "tw:glue" "r4" (func $r4 (param externref) (result i32)))
  (import "tw:glue" "r6" (func $r6 (param externref) (result i32)))
  (import "tw:glue" "r8" (func $r8 (param externref) (result i32)))
  (type $ft0 (func (param externref) (result anyref)))
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
  (table $entries (export "entries") 1 funcref)
  (func $f0 (export "f0") (param $p0 externref) (result i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r0 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
  )
  (func $f2 (export "f2") (param $p0 externref) (result i32 i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r2 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
    (i32.load offset=16 (i32.const 0))
  )
  (func $f4 (export "f4") (param $p0 externref) (result i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r4 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
  )
  (func $f6 (export "f6") (param $p0 externref) (result i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r6 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
  )
  (func $f8 (export "f8") (param $p0 externref) (result i32 i32 i32)
    (local $l1 i32)
    (local.set $l1 (call $r8 (local.get $p0)))
    local.get $l1
    (i32.load offset=8 (i32.const 0))
    (i32.load offset=16 (i32.const 0))
  )
  (func $h1 (param $p0 (ref null $d0)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 1)) (i32.const 1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i32.store8 (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 1))) (array.get_u $d0 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $c0 (export "c0") (param $p0 externref) (result i32 i32)
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (local $l3 i32)
    (local.set $l1 (ref.cast (ref null $d0) (any.convert_extern (local.get $p0))))
    local.get $l1
    call $h1
    local.set $l3
    local.set $l2
    local.get $l2
    local.get $l3
  )
  (func $h2 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $d0 (export "d0") (param $p0 i32) (param $p1 i32) (result externref)
    (local $l1 (ref null $d0))
    (local $l2 (ref $d0))
    local.get $p0
    local.get $p1
    call $h2
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    (extern.convert_any (local.get $l2))
  )
  (func $h3 (param $p0 (ref null $d0)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 1)) (i32.const 1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i32.store8 (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 1))) (array.get_u $d0 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $c1 (export "c1") (param $p0 externref) (result i32 i32)
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (local $l3 i32)
    (local.set $l1 (ref.cast (ref null $d0) (any.convert_extern (local.get $p0))))
    local.get $l1
    call $h3
    local.set $l3
    local.set $l2
    local.get $l2
    local.get $l3
  )
  (func $h4 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $d1 (export "d1") (param $p0 i32) (param $p1 i32) (result externref)
    (local $l1 (ref null $d0))
    local.get $p0
    local.get $p1
    call $h4
    local.set $l1
    (extern.convert_any (local.get $l1))
  )
  (func $h5 (param $p0 anyref) (result externref)
    (extern.convert_any (local.get $p0))
  )
  (func $e0 (export "e0") (param $p0 externref) (result externref)
    (local $l1 anyref)
    (local $l2 externref)
    local.get $p0
    (call_indirect $entries (type $ft0) (i32.const 0))
    local.set $l1
    local.get $l1
    call $h5
    local.set $l2
    local.get $l2
  )
)
