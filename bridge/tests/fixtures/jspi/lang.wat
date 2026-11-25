;; A hand-written language with no async of its own, which blocks on
;; futures under JSPI: it suspends in `tw:future suspend` and
;; `tw:future-list suspend`, and returns futures of its own.
(module
  (import "tw:js/global" "Api#later()" (func $later (param externref) (result externref)))
  (import "tw:future" "suspend" (func $suspend (param externref) (result i32)))
  (import "tw:future-list" "suspend" (func $race (param i32 i32) (result i32)))
  (import "tw:future" "get<u32>" (func $get (param externref) (result i32 i32)))
  (import "tw:future" "new" (func $new (result externref externref)))
  (import "tw:future" "resolve<u32>" (func $resolve (param externref i32)))
{RUNTIME}
  ;; A future of its own, resolved with `v`.
  (func $ready (param $v i32) (result externref)
    (local $fut externref) (local $res externref)
    (call $new)
    (local.set $res)
    (local.set $fut)
    (call $resolve (local.get $res) (local.get $v))
    (local.get $fut))

  ;; The value of a fulfilled future.
  (func $value (param $f externref) (result i32)
    (local $v i32)
    (call $get (local.get $f))
    (local.set $v)
    (if (i32.eqz) (then unreachable))
    (local.get $v))

  ;; Suspends until `f` is fulfilled, then reads it.
  (func $await (param $f externref) (result i32)
    (if (i32.ne (call $suspend (local.get $f)) (i32.const 1)) (then unreachable))
    (call $value (local.get $f)))

  (func (export "app.run(Api)") (param $api externref) (result externref)
    (local $a externref) (local $b externref)
    (local.set $a (call $later (local.get $api)))
    (local.set $b (call $later (local.get $api)))
    (call $ready (i32.add (call $await (local.get $a)) (call $await (local.get $b)))))

  ;; Awaits before reading `s`, so the bridge must keep the argument's
  ;; memory until the call settles.
  (func (export "app.echo(Api,str)") (param $api externref) (param $p i32) (param $n i32) (result externref)
    (local $v i32)
    (local.set $v (call $await (call $later (local.get $api))))
    (call $ready (i32.add (local.get $v) (i32.load8_u (local.get $p)))))

  ;; One future that several calls suspend on.
  (global $shared (mut externref) (ref.null extern))
  (func (export "app.share(Api)") (param $api externref) (result i32)
    (global.set $shared (call $later (local.get $api)))
    (i32.const 0))
  (func (export "app.peek()") (result externref)
    (call $ready (call $suspend (global.get $shared))))

  ;; Three futures in table slots 1 to 3, the list the suspension takes.
  (func $three (param $api externref)
    (table.set $tab (i32.const 1) (call $later (local.get $api)))
    (table.set $tab (i32.const 2) (call $later (local.get $api)))
    (table.set $tab (i32.const 3) (call $later (local.get $api))))
  (func $drop3
    (table.fill $tab (i32.const 1) (ref.null extern) (i32.const 3)))

  ;; The winner's index times 100, plus its value.
  (func (export "app.race(Api)") (param $api externref) (result externref)
    (local $i i32) (local $v i32)
    (call $three (local.get $api))
    (local.set $i (call $race (i32.const 1) (i32.const 3)))
    (local.set $v (call $value (table.get $tab (i32.add (i32.const 1) (local.get $i)))))
    (call $drop3)
    (call $ready (i32.add (i32.mul (local.get $i) (i32.const 100)) (local.get $v))))

  ;; Races the three once the third has settled.
  (func (export "app.lowest(Api)") (param $api externref) (result externref)
    (local $i i32)
    (call $three (local.get $api))
    (drop (call $suspend (table.get $tab (i32.const 3))))
    (local.set $i (call $race (i32.const 1) (i32.const 3)))
    (call $drop3)
    (call $ready (local.get $i)))

  (func (export "app.empty()") (result externref)
    (call $ready (call $race (i32.const 1) (i32.const 0))))

  ;; A callback returning a future: its environment is `n`.
  (func (export "app.plus(u32)") (param $n i32) (result i32) (local.get $n))
  (func (export "tw:call:fn(Api)->future<u32>") (param $env i32) (param $api externref) (result externref)
    (call $ready (i32.add (call $await (call $later (local.get $api))) (local.get $env))))
  (func (export "tw:drop:fn(Api)->future<u32>") (param $env i32))

  ;; A task's handle is the table slot holding its `Api`, which its drop
  ;; clears: one dropped while its run is suspended reads null.
  (global $drops (mut i32) (i32.const 0))
  (func (export "test:drops") (result i32) (global.get $drops))
  (func (export "Task.new(Api)") (param $self externref) (param $api externref) (result i32)
    (local $h i32)
    (local.set $h (call $talloc (i32.const 1)))
    (table.set $tab (local.get $h) (local.get $api))
    (local.get $h))
  (func (export "tw:drop:Task") (param $h i32)
    (global.set $drops (i32.add (global.get $drops) (i32.const 1)))
    (table.set $tab (local.get $h) (ref.null extern)))
  (func (export "Task#run()") (param $self externref) (param $h i32) (result externref)
    (local $v i32)
    ;; A linear language doesn't keep `this`.
    (local.set $self (ref.null extern))
    (local.set $v (call $await (call $later (table.get $tab (local.get $h)))))
    (if (ref.is_null (table.get $tab (local.get $h)))
      (then (local.set $v (i32.add (local.get $v) (i32.const 1000)))))
    (call $ready (local.get $v)))
)
