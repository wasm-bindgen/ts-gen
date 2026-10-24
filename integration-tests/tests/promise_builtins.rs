//! @ts-gen --experimental-generic-mono --fallible

#![cfg(target_arch = "wasm32")]

use js_sys::{JsString, Promise, TypeError};
use wasm_bindgen::JsValue;

use ts_gen_integration_tests::promise_builtins::{
    bun, cloudflare, count, fs, https, jsr, load, load_js_string, node, npm, scheduler, size,
    sleep, user_lib, Cache, Store,
};

// This fixture has no backing JavaScript module, so it provides compile-only
// coverage that under per-monomorphization every async callable is one
// `Promise<T, E>` import with no `_promise` twin, whether it belongs to a user
// module, a package, a URL, a builtin, a global or is declared `async`, and
// that `@throws` types `E` where `--fallible` would have typed the sync form.
#[allow(dead_code)]
async fn promise_builtins_compile(
    store: &Store,
    cache: &Cache,
    socket: &cloudflare::sockets::Socket,
) {
    // User modules: `--lib-name` and `declare module "user-lib"`.
    let _: Promise<String> = load("u");
    let _: Promise<JsString> = load_js_string("u");
    let _: Promise<f64, TypeError> = count("k");
    let _: Promise<String> = store.get("k");
    let _: Promise<JsString> = store.get_js_string("k");
    let _: Promise<String> = user_lib::fetch_text("u");

    // Package registry schemes and URLs.
    let _: Promise<String> = npm::lodash::fetch_text("u");
    let _: Promise<String> = jsr::std::path::fetch_text("u");
    let _: Promise<String> = https::esmsh::foo::fetch_text("u");

    // Declared `async`.
    let _: Promise<f64> = size("k");
    let _: Promise<()> = store.put("k", "v");
    let _: Promise<Store> = Store::open("s");

    // Globals.
    let _: Promise<()> = sleep(1.0);
    let _: Promise<Option<String>> = cache.match_("u");
    let _: Promise<Option<JsString>> = cache.match_js_string("u");
    let _: Promise<Cache> = Cache::open("c");
    let _: Promise<()> = scheduler::wait(1.0);

    // `@throws` types the rejection; the one binding awaits to `Result<T, E>`.
    let _: Promise<(), TypeError> = cache.put("u", "b");
    let _: Result<(), TypeError> = cache.put("u", "b").await;
    let _: Result<f64, TypeError> = count("k").await;
    let _: Result<String, JsValue> = load("u").await;

    // Builtin modules.
    let _: Promise<String> = node::fs::read_file("p");
    let _: Promise<String> = fs::read_file("p");
    let _: Promise<cloudflare::sockets::Socket> = cloudflare::sockets::connect("a");
    let _: Promise<()> = socket.close();
    let _: Promise<f64> = bun::sqlite::open("db");
}
