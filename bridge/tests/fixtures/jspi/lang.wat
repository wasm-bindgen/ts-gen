;; A hand-written language with no async of its own, which blocks on
;; futures under JSPI: it suspends in `tw:future suspend` and
;; `suspend_list`, and returns futures of its own.
(module
  (import "tw:js/global" "Api#later()" (func $later (param externref) (result externref)))
  (import "tw:future" "suspend" (func $suspend (param externref) (result i32)))
  (import "tw:future" "suspend_list" (func $race (param i32 i32) (result i32)))
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

  ;; Awaits before reading `s`: the argument is the language's, which
  ;; frees it once it's done with it.
  (func (export "app.echo(Api,str)") (param $api externref) (param $p i32) (param $n i32) (result externref)
    (local $v i32)
    (local.set $v (i32.add (call $await (call $later (local.get $api))) (i32.load8_u (local.get $p))))
    (call $free (local.get $p) (local.get $n) (i32.const 1))
    (call $ready (local.get $v)))

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

  ;; A handle is a counted block, `(count, value)`. JS holds one count, which
  ;; its drop releases, and a call takes its own to use the handle past a
  ;; suspension.
  (func $handle (param $v i32) (result i32)
    (local $h i32)
    (local.set $h (call $realloc (i32.const 0) (i32.const 0) (i32.const 4) (i32.const 8)))
    (i32.store (local.get $h) (i32.const 1))
    (i32.store offset=4 (local.get $h) (local.get $v))
    (local.get $h))
  (func $retain (param $h i32)
    (i32.store (local.get $h) (i32.add (i32.load (local.get $h)) (i32.const 1))))
  ;; 1 when the last count went, and the block with it.
  (func $release (param $h i32) (result i32)
    (local $n i32)
    (local.set $n (i32.sub (i32.load (local.get $h)) (i32.const 1)))
    (i32.store (local.get $h) (local.get $n))
    (if (local.get $n) (then (return (i32.const 0))))
    (call $free (local.get $h) (i32.const 8) (i32.const 4))
    (i32.const 1))
  (global $drops (mut i32) (i32.const 0))
  (global $freed (mut i32) (i32.const 0))
  (func (export "test:drops") (result i32) (global.get $drops))
  (func (export "test:freed") (result i32) (global.get $freed))
  (func $drop (param $h i32)
    (global.set $drops (i32.add (global.get $drops) (i32.const 1)))
    (global.set $freed (i32.add (global.get $freed) (call $release (local.get $h)))))

  ;; A callback returning a future: its environment holds `n`, read once
  ;; resumed.
  (func (export "app.plus(u32)") (param $n i32) (result i32) (call $handle (local.get $n)))
  (func (export "tw:call:fn(Api)->future<u32>") (param $env i32) (param $api externref) (result externref)
    (local $v i32)
    (call $retain (local.get $env))
    (local.set $v (i32.add (call $await (call $later (local.get $api))) (i32.load offset=4 (local.get $env))))
    (global.set $freed (i32.add (global.get $freed) (call $release (local.get $env))))
    (call $ready (local.get $v)))
  (func (export "tw:drop:fn(Api)->future<u32>") (param $env i32) (call $drop (local.get $env)))

  ;; A task holds the table slot of its `Api`, read on both sides of the
  ;; suspension.
  (func (export "Task.new(Api)") (param $self externref) (param $api externref) (result i32)
    (local $slot i32)
    (local.set $slot (call $talloc (i32.const 1)))
    (table.set $tab (local.get $slot) (local.get $api))
    (call $handle (local.get $slot)))
  (func (export "tw:drop:Task") (param $h i32) (call $drop (local.get $h)))
  (func (export "Task#run()") (param $self externref) (param $h i32) (result externref)
    (local $v i32)
    ;; A linear language doesn't keep `this`.
    (local.set $self (ref.null extern))
    (call $retain (local.get $h))
    (local.set $v (call $await (call $later (table.get $tab (i32.load offset=4 (local.get $h))))))
    ;; Freed under the call, the block would read as poison here.
    (if (ref.is_null (table.get $tab (i32.load offset=4 (local.get $h)))) (then unreachable))
    (global.set $freed (i32.add (global.get $freed) (call $release (local.get $h))))
    (call $ready (local.get $v)))
)
