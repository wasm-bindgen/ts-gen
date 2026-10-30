;; Tests variadic parameters: sole-rest (Console.log) and fixed+rest (joinAll),
;; and parameters named like the bindings' locals (restLog, arrLog).
(module
  (type $anyrefs (array (mut anyref)))

  ;; Global variable getter: console
  (import "./bindings.wasm" "console" (func $console (result externref)))
  ;; Sole-rest variadic instance method: Console.log(this, ...args)
  (import "./bindings.wasm" "Console.log" (func $Console.log (param externref (ref null $anyrefs))))
  ;; Fixed+rest variadic free function: joinAll(sep, ...parts)
  (import "./bindings.wasm" "joinAll" (func $joinAll (param externref (ref null $anyrefs)) (result externref)))
  (import "./bindings.wasm" "restLog" (func $restLog (param (ref null $anyrefs))))
  (import "./bindings.wasm" "arrLog" (func $arrLog (param externref (ref null $anyrefs))))

  (import "wasm:js/string-constants" "Hello" (global $hello externref))
  (import "wasm:js/string-constants" "World" (global $world externref))
  (import "wasm:js/string-constants" ", " (global $comma externref))

  (func (export "fetch") (param $req externref) (param $env externref) (param $ctx externref) (result externref)
    (local $arr (ref null $anyrefs))
    ;; Build array with two elements: "Hello", "World" (convert externref to anyref)
    (local.set $arr (array.new_fixed $anyrefs 2
      (any.convert_extern (global.get $hello))
      (any.convert_extern (global.get $world))))
    ;; Call console.log("Hello", "World") - sole-rest variadic via instance method
    (call $Console.log (call $console) (local.get $arr))
    (call $restLog (local.get $arr))
    (call $arrLog (global.get $comma) (local.get $arr))
    ;; Call joinAll(", ", "Hello", "World") => "Hello, World" - fixed+rest variadic
    (call $joinAll (global.get $comma) (local.get $arr)))
)
