//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Number, Promise, TypeError, Undefined};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::promise_builtins::{
    bun, cloudflare, count, count_promise, fs, load_promise, node, scheduler, size_promise,
    sleep_promise, user_lib, Cache, Store,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that under `--fallible` the `_promise` form of a user module's
// async callable catches a synchronous throw, while that of a builtin, a
// global or an `async`-declared callable is a plain `Promise`.
#[allow(dead_code)]
async fn promise_builtins_compile(
    store: &Store,
    cache: &Cache,
    socket: &cloudflare::sockets::Socket,
) {
    // User modules: `--lib-name` and `declare module "user-lib"`.
    let _: Result<Promise<JsString>, JsValue> = load_promise("u");
    let _: Result<Promise<Number>, TypeError> = count_promise("k");
    let _: Result<Promise<JsString>, JsValue> = store.get_promise("k");
    let _: Result<Promise<JsString>, JsValue> = user_lib::fetch_text_promise("u");

    // Declared `async`.
    let _: Promise<Number> = size_promise("k");
    let _: Promise<Undefined> = store.put_promise("k", "v");
    let _: Promise<Store> = Store::open_promise("s");

    // Globals.
    let _: Promise<Undefined> = sleep_promise(1.0);
    let _: Promise<Option<JsString>> = cache.match__promise("u");
    let _: Promise<Cache> = Cache::open_promise("c");
    let _: Promise<Undefined> = scheduler::wait_promise(1.0);

    // `@throws` on a builtin types the rejection only.
    let _: Promise<Undefined> = cache.put_promise("u", "b");
    let _: Result<(), TypeError> = cache.put("u", "b").await;
    let _: Result<f64, TypeError> = count("k").await;

    // Builtin modules.
    let _: Promise<JsString> = node::fs::read_file_promise("p");
    let _: Promise<JsString> = fs::read_file_promise("p");
    let _: Promise<cloudflare::sockets::Socket> = cloudflare::sockets::connect_promise("a");
    let _: Promise<Undefined> = socket.close_promise();
    let _: Promise<Number> = bun::sqlite::open_promise("db");
}
