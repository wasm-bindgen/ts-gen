;; A language under JSPI with a termination flag: `app.run` suspends on
;; `api.later()` inside a `catch_all`, and terminating while it's parked
;; stops it from resuming, so neither the handler nor the rest runs.
(module
  (import "tw:js/global" "Api#later()" (func $later (param externref) (result externref)))
  (import "tw:future" "suspend" (func $suspend (param externref) (result i32)))
  (import "tw:future" "get<u32>" (func $get (param externref) (result i32 i32)))
  (import "tw:future" "new" (func $new (result externref externref)))
  (import "tw:future" "resolve<u32>" (func $resolve (param externref i32)))
{RUNTIME}
  ;; The state byte (§15), below the allocator's first block.
  (global (export "tw:terminated") i32 (i32.const 8))
  (global $resumed (mut i32) (i32.const 0))
  (global $caught (mut i32) (i32.const 0))
  (func (export "test:resumed") (result i32) (global.get $resumed))
  (func (export "test:caught") (result i32) (global.get $caught))

  (func (export "app.run(Api)") (param $api externref) (result externref)
    (local $f externref) (local $v i32) (local $fut externref) (local $res externref)
    (local.set $f (call $later (local.get $api)))
    (block $done
      (block $catch
        (try_table (catch_all $catch)
          (drop (call $suspend (local.get $f))))
        (br $done))
      (global.set $caught (i32.add (global.get $caught) (i32.const 1))))
    (global.set $resumed (i32.add (global.get $resumed) (i32.const 1)))
    (call $get (local.get $f))
    (local.set $v)
    drop
    (call $new)
    (local.set $res)
    (local.set $fut)
    (call $resolve (local.get $res) (local.get $v))
    (local.get $fut))
)
