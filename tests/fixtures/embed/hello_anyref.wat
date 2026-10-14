;; Anyref mode: all references are anyref instead of externref.
;; Tests that embed accepts uniformly anyref-typed modules.
(module
  (import "./bindings.wasm" "Request.url" (func $Request.url (param anyref) (result anyref)))
  (import "./bindings.wasm" "Request.method" (func $Request.method (param anyref) (result anyref)))
  (import "./bindings.wasm" "Request.headers" (func $Request.headers (param anyref) (result anyref)))
  (import "./bindings.wasm" "Request.text" (func $Request.text (param anyref) (result anyref)))
  (import "./bindings.wasm" "Headers.get" (func $Headers.get (param anyref anyref) (result anyref)))
  (import "./bindings.wasm" "Headers.new" (func $Headers.new (result anyref)))
  (import "./bindings.wasm" "Headers.set" (func $Headers.set (param anyref anyref anyref)))
  (import "./bindings.wasm" "ResponseInit.new" (func $ResponseInit.new (result anyref)))
  (import "./bindings.wasm" "ResponseInit.set_status" (func $ResponseInit.set_status (param anyref f64)))
  (import "./bindings.wasm" "ResponseInit.set_headers" (func $ResponseInit.set_headers (param anyref anyref)))
  (import "./bindings.wasm" "Response.new_with_body_and_init"
    (func $Response.new (param anyref anyref) (result anyref)))
  (import "./builtins.js" "await" (func $await (param anyref) (result anyref)))

  ;; The js-string builtins and string constants are externref by spec;
  ;; convert at their boundary rather than retype them.
  (import "wasm:js-string" "equals" (func $str.eq (param externref externref) (result i32)))
  (import "wasm:js-string" "concat" (func $str.concat (param externref externref) (result (ref extern))))
  (import "wasm:js/string-constants" "GET" (global $GET externref))
  (import "wasm:js/string-constants" "Hello from Wasm!" (global $hello externref))
  (import "wasm:js/string-constants" "content-type" (global $content_type externref))
  (import "wasm:js/string-constants" "text/plain" (global $text_plain externref))
  (import "wasm:js/string-constants" "x-name" (global $x_name externref))
  (import "wasm:js/string-constants" ": " (global $colon externref))

  (func $respond (param $body anyref) (param $status f64) (result anyref)
    (local $init anyref) (local $headers anyref)
    (local.set $headers (call $Headers.new))
    (call $Headers.set (local.get $headers)
      (any.convert_extern (global.get $content_type)) (any.convert_extern (global.get $text_plain)))
    (local.set $init (call $ResponseInit.new))
    (call $ResponseInit.set_status (local.get $init) (local.get $status))
    (call $ResponseInit.set_headers (local.get $init) (local.get $headers))
    (call $Response.new (local.get $body) (local.get $init)))

  (func (export "fetch") (param $req anyref) (param $env anyref) (param $ctx anyref) (result anyref)
    (local $body anyref)
    (if (call $str.eq (extern.convert_any (call $Request.method (local.get $req))) (global.get $GET))
      (then (return (call $respond (any.convert_extern (global.get $hello)) (f64.const 200)))))
    ;; POST: echo "<x-name>: <body>", awaiting the body through JSPI
    (local.set $body (call $await (call $Request.text (local.get $req))))
    (call $respond
      (any.convert_extern
        (call $str.concat
          (call $str.concat
            (extern.convert_any
              (call $Headers.get (call $Request.headers (local.get $req)) (any.convert_extern (global.get $x_name))))
            (global.get $colon))
          (extern.convert_any (local.get $body))))
      (f64.const 200))))
