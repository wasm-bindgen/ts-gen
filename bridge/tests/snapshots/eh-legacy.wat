(module
  (type $d0 (array (mut i8)))
  (import "tw:glue" "g2" (func $g2 (param externref i32 i32) (result i32)))
  (import "tw:glue" "g3" (func $g3 (param externref i32 i32) (result i32)))
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
  (func $h2 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f2 (export "f2") (param $p0 externref) (param $p1 (ref $d0)) (result (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 (ref null $d0))
    (local $l106 (ref $d0))
    (local $l107 (ref null $d0))
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    try
      local.get $p1
      call $h1
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g2
      local.set $l103
      (local.set $l104 (i32.load offset=8 (i32.const 0)))
      local.get $l103
      local.get $l104
      call $h2
      local.set $l105
      (local.set $l106 (ref.as_non_null (local.get $l105)))
      (local.set $l107 (local.get $l106))
    catch_all
      (call $restore (local.get $l1) (local.get $l2))
      rethrow 0
    end
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l107))
  )
  (func $f3 (export "f3") (param $p0 externref) (param $p1 (ref $d0)) (result (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 (ref null $d0))
    (local $l106 (ref $d0))
    (local $l107 (ref null $d0))
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    try
      local.get $p1
      call $h1
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g3
      local.set $l103
      (local.set $l104 (i32.load offset=8 (i32.const 0)))
      local.get $l103
      local.get $l104
      call $h2
      local.set $l105
      (local.set $l106 (ref.as_non_null (local.get $l105)))
      (local.set $l107 (local.get $l106))
    catch_all
      (call $restore (local.get $l1) (local.get $l2))
      rethrow 0
    end
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l107))
  )
)
