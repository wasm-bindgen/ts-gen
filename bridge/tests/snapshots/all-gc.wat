(module
  (type $d0 (array (mut i8)))
  (type $d1 (array (mut f64)))
  (type $d2 (array (mut i64)))
  (type $d3 (array (mut i32)))
  (type $d4 (array (mut i16)))
  (type $d5 (array (mut (ref null $d0))))
  (type $d6 (struct (field i32) (field (ref $d0)) (field externref)))
  (type $d7 (array (mut (ref null $d6))))
  (type $d8 (array (mut (ref null $d5))))
  (type $d9 (struct (field i32) (field i64) (field f64)))
  (type $d10 (array (mut (ref null $d9))))
  (type $d11 (struct (field i32) (field (ref null $d0)) (field externref)))
  (type $d12 (array (mut (ref null $d11))))
  (import "tw:glue" "g0" (func $g0 (param externref) (result i32)))
  (import "tw:glue" "g1" (func $g1 (param externref i32 i32)))
  (import "tw:glue" "g2" (func $g2 (param externref) (result i32)))
  (import "tw:glue" "g3" (func $g3 (param externref i32 i32)))
  (import "tw:glue" "g4" (func $g4 (param externref) (result i32)))
  (import "tw:glue" "g5" (func $g5 (param externref i32 i32)))
  (import "tw:glue" "g6" (func $g6 (param externref) (result i32)))
  (import "tw:glue" "g7" (func $g7 (param externref i32 i32)))
  (import "tw:glue" "g8" (func $g8 (param externref) (result i32)))
  (import "tw:glue" "g9" (func $g9 (param externref i32 i32)))
  (import "tw:glue" "g10" (func $g10 (param externref) (result i32)))
  (import "tw:glue" "g11" (func $g11 (param externref i32 i32)))
  (import "tw:glue" "g12" (func $g12 (param externref) (result i32)))
  (import "tw:glue" "g13" (func $g13 (param externref i32 i32)))
  (import "tw:glue" "g14" (func $g14 (param externref) (result i32)))
  (import "tw:glue" "g15" (func $g15 (param externref i32 i32)))
  (import "tw:glue" "g16" (func $g16 (param externref) (result i32)))
  (import "tw:glue" "g17" (func $g17 (param externref i32 i32)))
  (import "tw:glue" "g18" (func $g18 (param externref) (result i32)))
  (import "tw:glue" "g19" (func $g19 (param externref i32 i32)))
  (import "tw:glue" "g20" (func $g20 (param externref) (result i32)))
  (import "tw:glue" "g21" (func $g21 (param externref i32 i32)))
  (import "tw:glue" "g22" (func $g22 (param externref) (result i32)))
  (import "tw:glue" "g23" (func $g23 (param externref i32 i32)))
  (import "tw:glue" "g24" (func $g24 (param externref) (result i32)))
  (import "tw:glue" "g25" (func $g25 (param externref i32 i32)))
  (import "tw:glue" "g26" (func $g26 (param externref) (result i32)))
  (import "tw:glue" "g27" (func $g27 (param externref i32 i32)))
  (import "tw:glue" "g28" (func $g28 (param externref) (result i32)))
  (import "tw:glue" "g29" (func $g29 (param externref i32 i32)))
  (import "tw:glue" "g30" (func $g30 (param externref) (result i32)))
  (import "tw:glue" "g31" (func $g31 (param externref i32 i32)))
  (import "tw:glue" "g32" (func $g32 (param externref) (result i32)))
  (import "tw:glue" "g33" (func $g33 (param externref i32 i32 i32)))
  (import "tw:glue" "g34" (func $g34 (param externref) (result i32)))
  (import "tw:glue" "g35" (func $g35 (param externref i32 i32)))
  (import "tw:glue" "g36" (func $g36 (param externref) (result i32)))
  (import "tw:glue" "g37" (func $g37 (param externref i32 i32)))
  (import "tw:glue" "g38" (func $g38 (param externref) (result i32)))
  (import "tw:glue" "g39" (func $g39 (param externref i32 i32 i32)))
  (import "tw:glue" "g40" (func $g40 (param externref) (result i32)))
  (import "tw:glue" "g41" (func $g41 (param externref i32 i32 i32 i32)))
  (import "tw:glue" "g42" (func $g42 (param externref) (result externref)))
  (import "tw:glue" "g43" (func $g43 (param externref i32 i32 i32 i32 i32 externref)))
  (import "tw:glue" "g44" (func $g44 (param externref) (result externref)))
  (import "tw:glue" "g45" (func $g45 (param externref i32 i32 i32 i32 i32 externref)))
  (import "tw:glue" "g46" (func $g46 (param externref) (result externref)))
  (import "tw:glue" "g47" (func $g47 (param externref i32 f64 i32 i32 i32 i32 i32 externref)))
  (import "tw:glue" "g50" (func $g50 (param externref) (result i32)))
  (import "tw:glue" "g51" (func $g51 (param externref i32 i32 i32)))
  (import "tw:glue" "g52" (func $g52 (param externref i32 i32)))
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
  (func $h1 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f0 (export "f0") (param $p0 externref) (result (ref $d0))
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
      call $g0
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
  (func $h2 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f1 (export "f1") (param $p0 externref) (param $p1 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h2
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g1
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h3 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f2 (export "f2") (param $p0 externref) (result (ref null $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d0))
    (local $l104 (ref null $d0))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g2
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h3
      local.set $l103
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
  )
  (func $h4 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f3 (export "f3") (param $p0 externref) (param $p1 (ref null $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h4
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g3
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h5 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f4 (export "f4") (param $p0 externref) (result (ref $d0))
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
      call $g4
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h5
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h6 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f5 (export "f5") (param $p0 externref) (param $p1 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h6
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g5
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h7 (param $p0 i32) (param $p1 i32) (result (ref null $d1))
    (local $l1 (ref null $d1))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d1))))
    (local.set $l1 (array.new_default $d1 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d1 (local.get $l1) (local.get $l2) (f64.load (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f6 (export "f6") (param $p0 externref) (result (ref $d1))
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
      call $g6
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h7
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h8 (param $p0 (ref null $d1)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 8)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (f64.store (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 8))) (array.get $d1 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f7 (export "f7") (param $p0 externref) (param $p1 (ref $d1))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h8
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g7
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h9 (param $p0 i32) (param $p1 i32) (result (ref null $d2))
    (local $l1 (ref null $d2))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d2))))
    (local.set $l1 (array.new_default $d2 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d2 (local.get $l1) (local.get $l2) (i64.load (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f8 (export "f8") (param $p0 externref) (result (ref $d2))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d2))
    (local $l104 (ref $d2))
    (local $l105 (ref null $d2))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g8
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
  (func $h10 (param $p0 (ref null $d2)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 8)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i64.store (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 8))) (array.get $d2 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f9 (export "f9") (param $p0 externref) (param $p1 (ref $d2))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h10
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g9
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h11 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f10 (export "f10") (param $p0 externref) (result (ref null $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d0))
    (local $l104 (ref null $d0))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g10
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h11
      local.set $l103
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
  )
  (func $h12 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f11 (export "f11") (param $p0 externref) (param $p1 (ref null $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h12
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g11
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h13 (param $p0 i32) (param $p1 i32) (result (ref null $d3))
    (local $l1 (ref null $d3))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d3))))
    (local.set $l1 (array.new_default $d3 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d3 (local.get $l1) (local.get $l2) (i32.load (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 4))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f12 (export "f12") (param $p0 externref) (result (ref $d3))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d3))
    (local $l104 (ref $d3))
    (local $l105 (ref null $d3))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g12
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h13
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h14 (param $p0 (ref null $d3)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 4)) (i32.const 4)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i32.store (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 4))) (array.get $d3 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f13 (export "f13") (param $p0 externref) (param $p1 (ref $d3))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h14
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g13
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h15 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f14 (export "f14") (param $p0 externref) (result (ref $d0))
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
      call $g14
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h15
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h16 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f15 (export "f15") (param $p0 externref) (param $p1 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h16
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g15
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h17 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f16 (export "f16") (param $p0 externref) (result (ref $d0))
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
      call $g16
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h17
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h18 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f17 (export "f17") (param $p0 externref) (param $p1 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h18
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g17
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h19 (param $p0 i32) (param $p1 i32) (result (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d0))))
    (local.set $l1 (array.new_default $d0 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d0 (local.get $l1) (local.get $l2) (i32.load8_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 1))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f18 (export "f18") (param $p0 externref) (result (ref $d0))
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
      call $g18
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h19
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h20 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $f19 (export "f19") (param $p0 externref) (param $p1 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h20
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g19
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h21 (param $p0 i32) (param $p1 i32) (result (ref null $d4))
    (local $l1 (ref null $d4))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d4))))
    (local.set $l1 (array.new_default $d4 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d4 (local.get $l1) (local.get $l2) (i32.load16_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 2))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f20 (export "f20") (param $p0 externref) (result (ref $d4))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d4))
    (local $l104 (ref $d4))
    (local $l105 (ref null $d4))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g20
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h21
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h22 (param $p0 (ref null $d4)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 2)) (i32.const 2)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i32.store16 (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 2))) (array.get_u $d4 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f21 (export "f21") (param $p0 externref) (param $p1 (ref $d4))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h22
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g21
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h23 (param $p0 i32) (param $p1 i32) (result (ref null $d4))
    (local $l1 (ref null $d4))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d4))))
    (local.set $l1 (array.new_default $d4 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d4 (local.get $l1) (local.get $l2) (i32.load16_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 2))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f22 (export "f22") (param $p0 externref) (result (ref $d4))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d4))
    (local $l104 (ref $d4))
    (local $l105 (ref null $d4))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g22
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h23
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h24 (param $p0 (ref null $d4)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 2)) (i32.const 2)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i32.store16 (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 2))) (array.get_u $d4 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f23 (export "f23") (param $p0 externref) (param $p1 (ref $d4))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h24
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g23
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h25 (param $p0 i32) (param $p1 i32) (result (ref null $d4))
    (local $l1 (ref null $d4))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d4))))
    (local.set $l1 (array.new_default $d4 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d4 (local.get $l1) (local.get $l2) (i32.load16_u (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 2))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f24 (export "f24") (param $p0 externref) (result (ref $d4))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d4))
    (local $l104 (ref $d4))
    (local $l105 (ref null $d4))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g24
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h25
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h26 (param $p0 (ref null $d4)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 2)) (i32.const 2)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i32.store16 (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 2))) (array.get_u $d4 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f25 (export "f25") (param $p0 externref) (param $p1 (ref $d4))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h26
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g25
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h27 (param $p0 i32) (param $p1 i32) (result (ref null $d2))
    (local $l1 (ref null $d2))
    (local $l2 i32)
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d2))))
    (local.set $l1 (array.new_default $d2 (local.get $p1)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l2) (local.get $p1))) (array.set $d2 (local.get $l1) (local.get $l2) (i64.load (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))) (local.set $l2 (i32.add (local.get $l2) (i32.const 1))) (br $l)))
    local.get $l1
  )
  (func $f26 (export "f26") (param $p0 externref) (result (ref $d2))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d2))
    (local $l104 (ref $d2))
    (local $l105 (ref null $d2))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g26
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h27
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h28 (param $p0 (ref null $d2)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 8)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l3) (local.get $l1))) (i64.store (i32.add (local.get $l2) (i32.mul (local.get $l3) (i32.const 8))) (array.get $d2 (local.get $p0) (local.get $l3))) (local.set $l3 (i32.add (local.get $l3) (i32.const 1))) (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f27 (export "f27") (param $p0 externref) (param $p1 (ref $d2))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h28
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g27
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h29 (param $p0 i32) (param $p1 i32) (result (ref null $d5))
    (local $l1 (ref null $d5))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d0))
    (local $l8 (ref $d0))
    (local $l9 (ref null $d0))
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d5))))
    (if (i32.eqz (local.get $p1)) (then (return (array.new_fixed $d5 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p1)))
    (local.set $l3 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))
    (local.set $l4 (i32.add (i32.const 0) (i32.mul (local.get $l2) (i32.const 0))))
    (local.set $l5 (i32.load offset=0 (local.get $l3)))
    (local.set $l6 (i32.load offset=4 (local.get $l3)))
    local.get $l5
    local.get $l6
    call $h1
    local.set $l7
    (local.set $l8 (ref.as_non_null (local.get $l7)))
    local.get $l8
    local.set $l9
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d5 (local.get $l9) (local.get $p1)))) (else (array.set $d5 (local.get $l1) (local.get $l2) (local.get $l9))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f28 (export "f28") (param $p0 externref) (result (ref $d5))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d5))
    (local $l104 (ref $d5))
    (local $l105 (ref null $d5))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g28
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h29
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h30 (param $p0 (ref null $d5)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d0))
    (local $l8 i32)
    (local $l9 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 4)))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 8))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 0))))
    (array.get $d5 (local.get $p0) (local.get $l4))
    local.set $l7
    local.get $l7
    call $h2
    local.set $l9
    local.set $l8
    (i32.store offset=0 (local.get $l5) (local.get $l8))
    (i32.store offset=4 (local.get $l5) (local.get $l9))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f29 (export "f29") (param $p0 externref) (param $p1 (ref $d5))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h30
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g29
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h31 (param $p0 i32) (param $p1 i32) (result (ref null $d5))
    (local $l1 (ref null $d5))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d0))
    (local $l8 (ref null $d0))
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d5))))
    (if (i32.eqz (local.get $p1)) (then (return (array.new_fixed $d5 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p1)))
    (local.set $l3 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))
    (local.set $l4 (i32.add (i32.const 0) (i32.mul (local.get $l2) (i32.const 0))))
    (local.set $l5 (i32.load offset=0 (local.get $l3)))
    (local.set $l6 (i32.load offset=4 (local.get $l3)))
    local.get $l5
    local.get $l6
    call $h3
    local.set $l7
    local.get $l7
    local.set $l8
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d5 (local.get $l8) (local.get $p1)))) (else (array.set $d5 (local.get $l1) (local.get $l2) (local.get $l8))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f30 (export "f30") (param $p0 externref) (result (ref null $d5))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d5))
    (local $l104 (ref null $d5))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g30
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h31
      local.set $l103
      (local.set $l104 (local.get $l103))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l104
  )
  (func $h32 (param $p0 (ref null $d5)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d0))
    (local $l8 i32)
    (local $l9 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 4)))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 8))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 0))))
    (array.get $d5 (local.get $p0) (local.get $l4))
    local.set $l7
    local.get $l7
    call $h4
    local.set $l9
    local.set $l8
    (i32.store offset=0 (local.get $l5) (local.get $l8))
    (i32.store offset=4 (local.get $l5) (local.get $l9))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f31 (export "f31") (param $p0 externref) (param $p1 (ref null $d5))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h32
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g31
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h34 (param $p0 i32) (param $p1 i32) (param $p2 i32) (param $p3 externref) (result i32 (ref null $d0) externref)
    (local $l1 (ref null $d0))
    (local $l2 (ref $d0))
    local.get $p1
    local.get $p2
    call $h1
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    local.get $p0
    local.get $l2
    local.get $p3
  )
  (func $h33 (param $p0 i32) (param $p1 i32) (param $p2 i32) (result (ref null $d7))
    (local $l1 (ref null $d7))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 i32)
    (local $l8 externref)
    (local $l9 i32)
    (local $l10 (ref null $d0))
    (local $l11 externref)
    (local $l12 (ref $d0))
    (local $l13 (ref null $d6))
    (if (i32.eq (local.get $p2) (i32.const -1)) (then (return (ref.null $d7))))
    (if (i32.eqz (local.get $p2)) (then (return (array.new_fixed $d7 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p2)))
    (local.set $l3 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 12))))
    (local.set $l4 (i32.add (local.get $p1) (i32.mul (local.get $l2) (i32.const 1))))
    (local.set $l5 (i32.load offset=0 (local.get $l3)))
    (local.set $l6 (i32.load offset=4 (local.get $l3)))
    (local.set $l7 (i32.load offset=8 (local.get $l3)))
    (local.set $l8 (table.get $st (i32.add (local.get $l4) (i32.const 0))))
    local.get $l5
    local.get $l6
    local.get $l7
    local.get $l8
    call $h34
    local.set $l11
    local.set $l10
    local.set $l9
    (local.set $l12 (ref.as_non_null (local.get $l10)))
    local.get $l9
    local.get $l12
    local.get $l11
    struct.new $d6
    local.set $l13
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d7 (local.get $l13) (local.get $p2)))) (else (array.set $d7 (local.get $l1) (local.get $l2) (local.get $l13))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f32 (export "f32") (param $p0 externref) (result (ref $d7))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 (ref null $d7))
    (local $l105 (ref $d7))
    (local $l106 (ref null $d7))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g32
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      (local.set $l103 (i32.load offset=16 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      call $h33
      local.set $l104
      (local.set $l105 (ref.as_non_null (local.get $l104)))
      (local.set $l106 (local.get $l105))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l106))
  )
  (func $h36 (param $p0 i32) (param $p1 (ref null $d0)) (param $p2 externref) (result i32 i32 i32 externref)
    (local $l1 i32)
    (local $l2 i32)
    local.get $p1
    call $h2
    local.set $l2
    local.set $l1
    local.get $p0
    local.get $l1
    local.get $l2
    local.get $p2
  )
  (func $h35 (param $p0 (ref null $d7)) (result i32 i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d6))
    (local $l8 i32)
    (local $l9 (ref $d0))
    (local $l10 externref)
    (local $l11 i32)
    (local $l12 i32)
    (local $l13 i32)
    (local $l14 externref)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 12)) (i32.const 4)))
    (local.set $l3 (call $stalloc (i32.mul (local.get $l1) (i32.const 1))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 12))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 1))))
    (array.get $d7 (local.get $p0) (local.get $l4))
    local.set $l7
    (local.set $l8 (struct.get $d6 0 (local.get $l7)))
    (local.set $l9 (struct.get $d6 1 (local.get $l7)))
    (local.set $l10 (struct.get $d6 2 (local.get $l7)))
    local.get $l8
    local.get $l9
    local.get $l10
    call $h36
    local.set $l14
    local.set $l13
    local.set $l12
    local.set $l11
    (i32.store offset=0 (local.get $l5) (local.get $l11))
    (i32.store offset=4 (local.get $l5) (local.get $l12))
    (i32.store offset=8 (local.get $l5) (local.get $l13))
    (table.set $st (i32.add (local.get $l6) (i32.const 0)) (local.get $l14))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l2
    local.get $l3
    local.get $l1
  )
  (func $f33 (export "f33") (param $p0 externref) (param $p1 (ref $d7))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h35
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      call $g33
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h37 (param $p0 i32) (param $p1 i32) (result (ref null $d8))
    (local $l1 (ref null $d8))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d5))
    (local $l8 (ref $d5))
    (local $l9 (ref null $d5))
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d8))))
    (if (i32.eqz (local.get $p1)) (then (return (array.new_fixed $d8 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p1)))
    (local.set $l3 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 8))))
    (local.set $l4 (i32.add (i32.const 0) (i32.mul (local.get $l2) (i32.const 0))))
    (local.set $l5 (i32.load offset=0 (local.get $l3)))
    (local.set $l6 (i32.load offset=4 (local.get $l3)))
    local.get $l5
    local.get $l6
    call $h29
    local.set $l7
    (local.set $l8 (ref.as_non_null (local.get $l7)))
    local.get $l8
    local.set $l9
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d8 (local.get $l9) (local.get $p1)))) (else (array.set $d8 (local.get $l1) (local.get $l2) (local.get $l9))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f34 (export "f34") (param $p0 externref) (result (ref $d8))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d8))
    (local $l104 (ref $d8))
    (local $l105 (ref null $d8))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g34
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h37
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h38 (param $p0 (ref null $d8)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d5))
    (local $l8 i32)
    (local $l9 i32)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 8)) (i32.const 4)))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 8))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 0))))
    (array.get $d8 (local.get $p0) (local.get $l4))
    local.set $l7
    local.get $l7
    call $h30
    local.set $l9
    local.set $l8
    (i32.store offset=0 (local.get $l5) (local.get $l8))
    (i32.store offset=4 (local.get $l5) (local.get $l9))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f35 (export "f35") (param $p0 externref) (param $p1 (ref $d8))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h38
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g35
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h39 (param $p0 i32) (param $p1 i32) (result (ref null $d10))
    (local $l1 (ref null $d10))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i64)
    (local $l7 f64)
    (local $l8 (ref null $d9))
    (if (i32.eq (local.get $p1) (i32.const -1)) (then (return (ref.null $d10))))
    (if (i32.eqz (local.get $p1)) (then (return (array.new_fixed $d10 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p1)))
    (local.set $l3 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 18))))
    (local.set $l4 (i32.add (i32.const 0) (i32.mul (local.get $l2) (i32.const 0))))
    (local.set $l5 (i32.load8_u offset=0 (local.get $l3)))
    (local.set $l6 (if (result i64) (i32.load8_u offset=1 (local.get $l3)) (then (i64.load offset=2 (local.get $l3))) (else (i64.const -9223372036854775808))))
    (local.set $l7 (f64.load offset=10 (local.get $l3)))
    local.get $l5
    local.get $l6
    local.get $l7
    struct.new $d9
    local.set $l8
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d10 (local.get $l8) (local.get $p1)))) (else (array.set $d10 (local.get $l1) (local.get $l2) (local.get $l8))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f36 (export "f36") (param $p0 externref) (result (ref $d10))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 (ref null $d10))
    (local $l104 (ref $d10))
    (local $l105 (ref null $d10))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g36
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      local.get $l101
      local.get $l102
      call $h39
      local.set $l103
      (local.set $l104 (ref.as_non_null (local.get $l103)))
      (local.set $l105 (local.get $l104))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l105))
  )
  (func $h40 (param $p0 (ref null $d10)) (result i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d9))
    (local $l8 i32)
    (local $l9 i64)
    (local $l10 f64)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 18)) (i32.const 8)))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 18))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 0))))
    (array.get $d10 (local.get $p0) (local.get $l4))
    local.set $l7
    (local.set $l8 (struct.get $d9 0 (local.get $l7)))
    (local.set $l9 (struct.get $d9 1 (local.get $l7)))
    (local.set $l10 (struct.get $d9 2 (local.get $l7)))
    (i32.store8 offset=0 (local.get $l5) (local.get $l8))
    (if (i64.eq (local.get $l9) (i64.const -9223372036854775808)) (then (i32.store8 offset=1 (local.get $l5) (i32.const 0)) (i64.store offset=2 (local.get $l5) (i64.const 0))) (else (i32.store8 offset=1 (local.get $l5) (i32.const 1)) (i64.store offset=2 (local.get $l5) (local.get $l9))))
    (f64.store offset=10 (local.get $l5) (local.get $l10))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l2
    local.get $l1
  )
  (func $f37 (export "f37") (param $p0 externref) (param $p1 (ref $d10))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h40
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g37
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h41 (param $p0 i32) (param $p1 i32) (param $p2 i32) (result i32 (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 (ref $d0))
    local.get $p1
    local.get $p2
    call $h1
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    local.get $p0
    local.get $l2
  )
  (func $f38 (export "f38") (param $p0 externref) (result i32 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 (ref null $d0))
    (local $l106 (ref $d0))
    (local $l107 i32)
    (local $l108 (ref null $d0))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g38
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      (local.set $l103 (i32.load offset=16 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      call $h41
      local.set $l105
      local.set $l104
      (local.set $l106 (ref.as_non_null (local.get $l105)))
      (local.set $l107 (local.get $l104))
      (local.set $l108 (local.get $l106))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l107
    (ref.as_non_null (local.get $l108))
  )
  (func $h42 (param $p0 i32) (param $p1 (ref null $d0)) (result i32 i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    local.get $p1
    call $h2
    local.set $l2
    local.set $l1
    local.get $p0
    local.get $l1
    local.get $l2
  )
  (func $f39 (export "f39") (param $p0 externref) (param $p1 i32) (param $p2 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      local.get $p2
      call $h42
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      call $g39
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h43 (param $p0 i32) (param $p1 i32) (param $p2 i32) (param $p3 i32) (result i32 i32 (ref null $d0))
    (local $l1 (ref null $d0))
    (local $l2 (ref $d0))
    local.get $p2
    local.get $p3
    call $h1
    local.set $l1
    (local.set $l2 (ref.as_non_null (local.get $l1)))
    local.get $p0
    local.get $p1
    local.get $l2
  )
  (func $f40 (export "f40") (param $p0 externref) (result i32 i32 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 i32)
    (local $l107 (ref null $d0))
    (local $l108 (ref $d0))
    (local $l109 i32)
    (local $l110 i32)
    (local $l111 (ref null $d0))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g40
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      (local.set $l103 (i32.load offset=16 (i32.const 0)))
      (local.set $l104 (i32.load offset=24 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      call $h43
      local.set $l107
      local.set $l106
      local.set $l105
      (local.set $l108 (ref.as_non_null (local.get $l107)))
      (local.set $l109 (local.get $l105))
      (local.set $l110 (local.get $l106))
      (local.set $l111 (local.get $l108))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l109
    local.get $l110
    (ref.as_non_null (local.get $l111))
  )
  (func $h44 (param $p0 i32) (param $p1 i32) (param $p2 (ref null $d0)) (result i32 i32 i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    local.get $p2
    call $h2
    local.set $l2
    local.set $l1
    local.get $p0
    local.get $p1
    local.get $l1
    local.get $l2
  )
  (func $f41 (export "f41") (param $p0 externref) (param $p1 i32) (param $p2 i32) (param $p3 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      local.get $p2
      local.get $p3
      call $h44
      local.set $l104
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      call $g41
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h45 (param $p0 i32) (param $p1 i32) (param $p2 i32) (param $p3 i32) (param $p4 i32) (param $p5 externref) (result i32 (ref null $d0) externref)
    (local $l1 (ref null $d0))
    (local $l2 externref)
    (local $l3 (ref null $d0))
    (local $l4 (ref $d0))
    (local $l5 (ref null $d0))
    (local $l6 (ref $d0))
    (if (i32.eq (local.get $p0) (i32.const 1)) (then
    local.get $p1
    local.get $p2
    call $h1
    local.set $l3
    (local.set $l4 (ref.as_non_null (local.get $l3)))
    (local.set $l1 (local.get $l4))
    ))
    (if (i32.eq (local.get $p0) (i32.const 2)) (then
    local.get $p3
    local.get $p4
    call $h5
    local.set $l5
    (local.set $l6 (ref.as_non_null (local.get $l5)))
    (local.set $l1 (local.get $l6))
    ))
    (if (i32.eq (local.get $p0) (i32.const 3)) (then
    (local.set $l2 (local.get $p5))
    ))
    local.get $p0
    local.get $l1
    local.get $l2
  )
  (func $f42 (export "f42") (param $p0 externref) (result i32 (ref null $d0) externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 externref)
    (local $l107 i32)
    (local $l108 (ref null $d0))
    (local $l109 externref)
    (local $l110 i32)
    (local $l111 (ref null $d0))
    (local $l112 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g42
      local.set $l106
      (local.set $l101 (i32.load offset=8 (i32.const 0)))
      (local.set $l102 (i32.load offset=16 (i32.const 0)))
      (local.set $l103 (i32.load offset=24 (i32.const 0)))
      (local.set $l104 (i32.load offset=32 (i32.const 0)))
      (local.set $l105 (i32.load offset=40 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      local.get $l105
      local.get $l106
      call $h45
      local.set $l109
      local.set $l108
      local.set $l107
      (local.set $l110 (local.get $l107))
      (local.set $l111 (local.get $l108))
      (local.set $l112 (local.get $l109))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l110
    local.get $l111
    local.get $l112
  )
  (func $h46 (param $p0 i32) (param $p1 (ref null $d0)) (param $p2 externref) (result i32 i32 i32 i32 i32 externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 externref)
    (local $l7 i32)
    (local $l8 i32)
    (local $l9 i32)
    (local $l10 i32)
    (local.set $l1 (local.get $p0))
    (if (i32.eq (local.get $p0) (i32.const 1)) (then
    local.get $p1
    call $h2
    local.set $l8
    local.set $l7
    (local.set $l2 (local.get $l7))
    (local.set $l3 (local.get $l8))
    ))
    (if (i32.eq (local.get $p0) (i32.const 2)) (then
    local.get $p1
    call $h6
    local.set $l10
    local.set $l9
    (local.set $l4 (local.get $l9))
    (local.set $l5 (local.get $l10))
    ))
    (if (i32.eq (local.get $p0) (i32.const 3)) (then
    (local.set $l6 (local.get $p2))
    ))
    local.get $l1
    local.get $l2
    local.get $l3
    local.get $l4
    local.get $l5
    local.get $l6
  )
  (func $f43 (export "f43") (param $p0 externref) (param $p1 i32) (param $p2 (ref null $d0)) (param $p3 externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      local.get $p2
      local.get $p3
      call $h46
      local.set $l106
      local.set $l105
      local.set $l104
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      local.get $l105
      local.get $l106
      call $g43
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h47 (param $p0 i32) (param $p1 i32) (param $p2 i32) (param $p3 i32) (param $p4 i32) (param $p5 externref) (result i32 (ref null $d0) externref)
    (local $l1 (ref null $d0))
    (local $l2 externref)
    (local $l3 (ref null $d0))
    (local $l4 (ref $d0))
    (local $l5 (ref null $d0))
    (local $l6 (ref $d0))
    (if (i32.eq (local.get $p0) (i32.const 1)) (then
    local.get $p1
    local.get $p2
    call $h1
    local.set $l3
    (local.set $l4 (ref.as_non_null (local.get $l3)))
    (local.set $l1 (local.get $l4))
    ))
    (if (i32.eq (local.get $p0) (i32.const 2)) (then
    local.get $p3
    local.get $p4
    call $h5
    local.set $l5
    (local.set $l6 (ref.as_non_null (local.get $l5)))
    (local.set $l1 (local.get $l6))
    ))
    (if (i32.eq (local.get $p0) (i32.const 3)) (then
    (local.set $l2 (local.get $p5))
    ))
    local.get $p0
    local.get $l1
    local.get $l2
  )
  (func $f44 (export "f44") (param $p0 externref) (result i32 (ref null $d0) externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 externref)
    (local $l107 i32)
    (local $l108 (ref null $d0))
    (local $l109 externref)
    (local $l110 i32)
    (local $l111 (ref null $d0))
    (local $l112 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g44
      local.set $l106
      (local.set $l101 (i32.load offset=8 (i32.const 0)))
      (local.set $l102 (i32.load offset=16 (i32.const 0)))
      (local.set $l103 (i32.load offset=24 (i32.const 0)))
      (local.set $l104 (i32.load offset=32 (i32.const 0)))
      (local.set $l105 (i32.load offset=40 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      local.get $l105
      local.get $l106
      call $h47
      local.set $l109
      local.set $l108
      local.set $l107
      (local.set $l110 (local.get $l107))
      (local.set $l111 (local.get $l108))
      (local.set $l112 (local.get $l109))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l110
    local.get $l111
    local.get $l112
  )
  (func $h48 (param $p0 i32) (param $p1 (ref null $d0)) (param $p2 externref) (result i32 i32 i32 i32 i32 externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 externref)
    (local $l7 i32)
    (local $l8 i32)
    (local $l9 i32)
    (local $l10 i32)
    (local.set $l1 (local.get $p0))
    (if (i32.eq (local.get $p0) (i32.const 1)) (then
    local.get $p1
    call $h2
    local.set $l8
    local.set $l7
    (local.set $l2 (local.get $l7))
    (local.set $l3 (local.get $l8))
    ))
    (if (i32.eq (local.get $p0) (i32.const 2)) (then
    local.get $p1
    call $h6
    local.set $l10
    local.set $l9
    (local.set $l4 (local.get $l9))
    (local.set $l5 (local.get $l10))
    ))
    (if (i32.eq (local.get $p0) (i32.const 3)) (then
    (local.set $l6 (local.get $p2))
    ))
    local.get $l1
    local.get $l2
    local.get $l3
    local.get $l4
    local.get $l5
    local.get $l6
  )
  (func $f45 (export "f45") (param $p0 externref) (param $p1 i32) (param $p2 (ref null $d0)) (param $p3 externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      local.get $p2
      local.get $p3
      call $h48
      local.set $l106
      local.set $l105
      local.set $l104
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      local.get $l105
      local.get $l106
      call $g45
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h49 (param $p0 i32) (param $p1 f64) (param $p2 i32) (param $p3 i32) (param $p4 i32) (param $p5 i32) (param $p6 i32) (param $p7 externref) (result i32 f64 i32 (ref null $d0) (ref null $d5) externref)
    (local $l1 f64)
    (local $l2 i32)
    (local $l3 (ref null $d0))
    (local $l4 (ref null $d5))
    (local $l5 externref)
    (local $l6 (ref null $d0))
    (local $l7 (ref $d0))
    (local $l8 (ref null $d5))
    (local $l9 (ref $d5))
    (if (i32.eq (local.get $p0) (i32.const 1)) (then
    (local.set $l1 (local.get $p1))
    ))
    (if (i32.eq (local.get $p0) (i32.const 2)) (then
    local.get $p3
    local.get $p4
    call $h1
    local.set $l6
    (local.set $l7 (ref.as_non_null (local.get $l6)))
    (local.set $l2 (local.get $p2))
    (local.set $l3 (local.get $l7))
    ))
    (if (i32.eq (local.get $p0) (i32.const 3)) (then
    local.get $p5
    local.get $p6
    call $h29
    local.set $l8
    (local.set $l9 (ref.as_non_null (local.get $l8)))
    (local.set $l4 (local.get $l9))
    ))
    (if (i32.eq (local.get $p0) (i32.const -1)) (then
    (local.set $l5 (local.get $p7))
    ))
    local.get $p0
    local.get $l1
    local.get $l2
    local.get $l3
    local.get $l4
    local.get $l5
  )
  (func $f46 (export "f46") (param $p0 externref) (result i32 f64 i32 (ref null $d0) (ref null $d5) externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 f64)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 i32)
    (local $l107 i32)
    (local $l108 externref)
    (local $l109 i32)
    (local $l110 f64)
    (local $l111 i32)
    (local $l112 (ref null $d0))
    (local $l113 (ref null $d5))
    (local $l114 externref)
    (local $l115 i32)
    (local $l116 f64)
    (local $l117 i32)
    (local $l118 (ref null $d0))
    (local $l119 (ref null $d5))
    (local $l120 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g46
      local.set $l108
      (local.set $l101 (i32.load offset=8 (i32.const 0)))
      (local.set $l102 (f64.load offset=16 (i32.const 0)))
      (local.set $l103 (i32.load offset=24 (i32.const 0)))
      (local.set $l104 (i32.load offset=32 (i32.const 0)))
      (local.set $l105 (i32.load offset=40 (i32.const 0)))
      (local.set $l106 (i32.load offset=48 (i32.const 0)))
      (local.set $l107 (i32.load offset=56 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      local.get $l105
      local.get $l106
      local.get $l107
      local.get $l108
      call $h49
      local.set $l114
      local.set $l113
      local.set $l112
      local.set $l111
      local.set $l110
      local.set $l109
      (local.set $l115 (local.get $l109))
      (local.set $l116 (local.get $l110))
      (local.set $l117 (local.get $l111))
      (local.set $l118 (local.get $l112))
      (local.set $l119 (local.get $l113))
      (local.set $l120 (local.get $l114))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    local.get $l115
    local.get $l116
    local.get $l117
    local.get $l118
    local.get $l119
    local.get $l120
  )
  (func $h50 (param $p0 i32) (param $p1 f64) (param $p2 i32) (param $p3 (ref null $d0)) (param $p4 (ref null $d5)) (param $p5 externref) (result i32 f64 i32 i32 i32 i32 i32 externref)
    (local $l1 i32)
    (local $l2 f64)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 i32)
    (local $l8 externref)
    (local $l9 i32)
    (local $l10 i32)
    (local $l11 i32)
    (local $l12 i32)
    (local.set $l1 (local.get $p0))
    (if (i32.eq (local.get $p0) (i32.const 1)) (then
    (local.set $l2 (local.get $p1))
    ))
    (if (i32.eq (local.get $p0) (i32.const 2)) (then
    local.get $p3
    call $h2
    local.set $l10
    local.set $l9
    (local.set $l3 (local.get $p2))
    (local.set $l4 (local.get $l9))
    (local.set $l5 (local.get $l10))
    ))
    (if (i32.eq (local.get $p0) (i32.const 3)) (then
    local.get $p4
    call $h30
    local.set $l12
    local.set $l11
    (local.set $l6 (local.get $l11))
    (local.set $l7 (local.get $l12))
    ))
    (if (i32.eq (local.get $p0) (i32.const -1)) (then
    (local.set $l8 (local.get $p5))
    ))
    local.get $l1
    local.get $l2
    local.get $l3
    local.get $l4
    local.get $l5
    local.get $l6
    local.get $l7
    local.get $l8
  )
  (func $f47 (export "f47") (param $p0 externref) (param $p1 i32) (param $p2 f64) (param $p3 i32) (param $p4 (ref null $d0)) (param $p5 (ref null $d5)) (param $p6 externref)
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 f64)
    (local $l103 i32)
    (local $l104 i32)
    (local $l105 i32)
    (local $l106 i32)
    (local $l107 i32)
    (local $l108 externref)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      local.get $p2
      local.get $p3
      local.get $p4
      local.get $p5
      local.get $p6
      call $h50
      local.set $l108
      local.set $l107
      local.set $l106
      local.set $l105
      local.set $l104
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      local.get $l104
      local.get $l105
      local.get $l106
      local.get $l107
      local.get $l108
      call $g47
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h51 (param $p0 i32) (param $p1 i32) (param $p2 i32) (result (ref null $d12))
    (local $l1 (ref null $d12))
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 i32)
    (local $l8 i32)
    (local $l9 i32)
    (local $l10 externref)
    (local $l11 i32)
    (local $l12 (ref null $d0))
    (local $l13 externref)
    (local $l14 (ref null $d11))
    (if (i32.eq (local.get $p2) (i32.const -1)) (then (return (ref.null $d12))))
    (if (i32.eqz (local.get $p2)) (then (return (array.new_fixed $d12 0))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l2) (local.get $p2)))
    (local.set $l3 (i32.add (local.get $p0) (i32.mul (local.get $l2) (i32.const 20))))
    (local.set $l4 (i32.add (local.get $p1) (i32.mul (local.get $l2) (i32.const 1))))
    (local.set $l5 (i32.load offset=0 (local.get $l3)))
    (local.set $l6 (i32.load offset=4 (local.get $l3)))
    (local.set $l7 (i32.load offset=8 (local.get $l3)))
    (local.set $l8 (i32.load offset=12 (local.get $l3)))
    (local.set $l9 (i32.load offset=16 (local.get $l3)))
    (local.set $l10 (table.get $st (i32.add (local.get $l4) (i32.const 0))))
    local.get $l5
    local.get $l6
    local.get $l7
    local.get $l8
    local.get $l9
    local.get $l10
    call $h45
    local.set $l13
    local.set $l12
    local.set $l11
    local.get $l11
    local.get $l12
    local.get $l13
    struct.new $d11
    local.set $l14
    (if (i32.eqz (local.get $l2)) (then (local.set $l1 (array.new $d12 (local.get $l14) (local.get $p2)))) (else (array.set $d12 (local.get $l1) (local.get $l2) (local.get $l14))))
    (local.set $l2 (i32.add (local.get $l2) (i32.const 1)))
    (br $l)))
    local.get $l1
  )
  (func $f50 (export "f50") (param $p0 externref) (result (ref $d12))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l104 (ref null $d12))
    (local $l105 (ref $d12))
    (local $l106 (ref null $d12))
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p0
      call $g50
      local.set $l101
      (local.set $l102 (i32.load offset=8 (i32.const 0)))
      (local.set $l103 (i32.load offset=16 (i32.const 0)))
      local.get $l101
      local.get $l102
      local.get $l103
      call $h51
      local.set $l104
      (local.set $l105 (ref.as_non_null (local.get $l104)))
      (local.set $l106 (local.get $l105))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
    (ref.as_non_null (local.get $l106))
  )
  (func $h52 (param $p0 (ref null $d12)) (result i32 i32 i32)
    (local $l1 i32)
    (local $l2 i32)
    (local $l3 i32)
    (local $l4 i32)
    (local $l5 i32)
    (local $l6 i32)
    (local $l7 (ref null $d11))
    (local $l8 i32)
    (local $l9 (ref null $d0))
    (local $l10 externref)
    (local $l11 i32)
    (local $l12 i32)
    (local $l13 i32)
    (local $l14 i32)
    (local $l15 i32)
    (local $l16 externref)
    (if (ref.is_null (local.get $p0)) (then (return (i32.const 0) (i32.const 0) (i32.const -1))))
    (local.set $l1 (array.len (local.get $p0)))
    (local.set $l2 (call $salloc (i32.mul (local.get $l1) (i32.const 20)) (i32.const 4)))
    (local.set $l3 (call $stalloc (i32.mul (local.get $l1) (i32.const 1))))
    (block $b (loop $l
    (br_if $b (i32.ge_u (local.get $l4) (local.get $l1)))
    (local.set $l5 (i32.add (local.get $l2) (i32.mul (local.get $l4) (i32.const 20))))
    (local.set $l6 (i32.add (local.get $l3) (i32.mul (local.get $l4) (i32.const 1))))
    (array.get $d12 (local.get $p0) (local.get $l4))
    local.set $l7
    (local.set $l8 (struct.get $d11 0 (local.get $l7)))
    (local.set $l9 (struct.get $d11 1 (local.get $l7)))
    (local.set $l10 (struct.get $d11 2 (local.get $l7)))
    local.get $l8
    local.get $l9
    local.get $l10
    call $h46
    local.set $l16
    local.set $l15
    local.set $l14
    local.set $l13
    local.set $l12
    local.set $l11
    (i32.store offset=0 (local.get $l5) (local.get $l11))
    (i32.store offset=4 (local.get $l5) (local.get $l12))
    (i32.store offset=8 (local.get $l5) (local.get $l13))
    (i32.store offset=12 (local.get $l5) (local.get $l14))
    (i32.store offset=16 (local.get $l5) (local.get $l15))
    (table.set $st (i32.add (local.get $l6) (i32.const 0)) (local.get $l16))
    (local.set $l4 (i32.add (local.get $l4) (i32.const 1)))
    (br $l)))
    local.get $l2
    local.get $l3
    local.get $l1
  )
  (func $f51 (export "f51") (param $p0 externref) (param $p1 (ref $d12))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l103 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h52
      local.set $l103
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      local.get $l103
      call $g51
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
  (func $h53 (param $p0 (ref null $d0)) (result i32 i32)
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
  (func $h54 (param $p0 (ref null $d0)) (param $p1 i32)
    (local $l1 i32)
    (local $l2 i32)
    (if (ref.is_null (local.get $p0)) (then (return)))
    (local.set $l2 (array.len (local.get $p0)))
    (block $b (loop $l (br_if $b (i32.ge_u (local.get $l1) (local.get $l2))) (array.set $d0 (local.get $p0) (local.get $l1) (i32.load8_u (i32.add (local.get $p1) (i32.mul (local.get $l1) (i32.const 1))))) (local.set $l1 (i32.add (local.get $l1) (i32.const 1))) (br $l)))
  )
  (func $f52 (export "f52") (param $p0 externref) (param $p1 (ref $d0))
    (local $l1 i32)
    (local $l2 i32)
    (local $l101 i32)
    (local $l102 i32)
    (local $l3 exnref)
    (local.set $l1 (call $sp))
    (local.set $l2 (call $tsp))
    (block $done
    (local.set $l3 (block $catch (result exnref) (try_table (catch_all_ref $catch)
      local.get $p1
      call $h53
      local.set $l102
      local.set $l101
      local.get $p0
      local.get $l101
      local.get $l102
      call $g52
      (call $h54 (local.get $p1) (local.get $l101))
    ) (br $done)))
    (call $restore (local.get $l1) (local.get $l2))
    (throw_ref (local.get $l3)))
    (call $restore (local.get $l1) (local.get $l2))
  )
)
