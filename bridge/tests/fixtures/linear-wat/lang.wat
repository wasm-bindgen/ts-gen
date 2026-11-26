;; A hand-written linear language: closure environments in its own memory,
;; a cleanup that runs while an exception unwinds through it, and a
;; termination state byte.
(module
  (import "tw:js/global" "Target#on(fn(u32)->u32)" (func $on (param externref i32)))
  (import "tw:js/global" "Target#fire(u32)" (func $fire (param externref i32) (result i32)))
  (import "tw:destroy" "fn(u32)->u32" (func $destroy (param i32)))
{RUNTIME}
  (global (export "tw:terminated") i32 (i32.const 8))
  (global $last (mut i32) (i32.const 0))
  (global $drops (mut i32) (i32.const 0))
  (func (export "test:drops") (result i32) (global.get $drops))

  ;; An environment holds the closure's captured `base`.
  (func (export "app.listen(Target,u32)") (param $t externref) (param $base i32)
    (local $env i32)
    (local.set $env (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 8)))
    (i32.store (local.get $env) (local.get $base))
    (global.set $last (local.get $env))
    (call $on (local.get $t) (local.get $env)))
  (func (export "tw:call:fn(u32)->u32") (param $env i32) (param $x i32) (result i32)
    (i32.add (i32.load (local.get $env)) (local.get $x)))
  (func (export "tw:drop:fn(u32)->u32") (param $env i32)
    (global.set $drops (i32.add (global.get $drops) (i32.const 1)))
    (call $free (local.get $env) (i32.const 8) (i32.const 4)))
  (func (export "app.forget()")
    (call $destroy (global.get $last))
    (call $free (global.get $last) (i32.const 8) (i32.const 4)))

  ;; A buffer and the string argument, both the language's, freed on both
  ;; the normal and the unwinding path.
  (func $cleanup (param $buf i32) (param $p i32) (param $n i32)
    (call $free (local.get $buf) (i32.const 16) (i32.const 1))
    (if (local.get $n) (then (call $free (local.get $p) (local.get $n) (i32.const 1)))))
  (func (export "app.guarded(Target,str)") (param $t externref) (param $p i32) (param $n i32) (result i32)
    (local $buf i32) (local $r i32) (local $e exnref)
    (local.set $buf (call $realloc (i32.const 0) (i32.const 0) (i32.const 1) (i32.const 16)))
    (block $done
      (local.set $e
        (block $catch (result exnref)
          (try_table (catch_all_ref $catch)
            (local.set $r (call $fire (local.get $t) (local.get $n))))
          (br $done)))
      (call $cleanup (local.get $buf) (local.get $p) (local.get $n))
      (throw_ref (local.get $e)))
    (call $cleanup (local.get $buf) (local.get $p) (local.get $n))
    (local.get $r))

  ;; An argument is the language's: kept past its call, and handed back as
  ;; a result.
  (global $kept (mut i32) (i32.const 0))
  (global $keptn (mut i32) (i32.const 0))
  (func (export "app.keep(str,Target)") (param $p i32) (param $n i32) (param externref)
    (global.set $kept (local.get $p))
    (global.set $keptn (local.get $n)))
  (func (export "app.kept()") (result i32 i32)
    (global.get $kept) (global.get $keptn))

  ;; A table range is the language's too, cleared before it's freed.
  (func (export "app.pick(arr<Target>,u32)") (param $base i32) (param $n i32) (param $i i32) (result externref)
    (local $t externref)
    (local.set $t (table.get $tab (i32.add (local.get $base) (local.get $i))))
    (if (local.get $n)
      (then
        (table.fill $tab (local.get $base) (ref.null extern) (local.get $n))
        (call $tfree (local.get $base) (local.get $n))))
    (local.get $t))

  (func (export "app.terminate()")
    (i32.store8 (i32.const 8) (i32.const 1)))
)
