#[allow(dead_code)]
use ::web_sys::Request;
#[allow(dead_code)]
use ::web_sys::RequestInit;
#[allow(dead_code)]
use ::web_sys::Response;
#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = RequestInit , extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type FetchOptions;
    #[doc = " Number of times to retry on failure."]
    #[wasm_bindgen(method, getter)]
    pub fn retries(this: &FetchOptions) -> Option<f64>;
    #[wasm_bindgen(method, setter)]
    pub fn set_retries(this: &FetchOptions, val: f64);
    #[doc = " Timeout in milliseconds."]
    #[wasm_bindgen(method, getter)]
    pub fn timeout(this: &FetchOptions) -> Option<f64>;
    #[wasm_bindgen(method, setter)]
    pub fn set_timeout(this: &FetchOptions, val: f64);
    #[doc = " Custom priority hint."]
    #[wasm_bindgen(method, getter)]
    pub fn priority(this: &FetchOptions) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_priority(this: &FetchOptions, val: &str);
}
impl FetchOptions {
    pub fn new() -> FetchOptions {
        Self::builder().build()
    }
    pub fn builder() -> FetchOptionsBuilder {
        FetchOptionsBuilder {
            inner: JsCast::unchecked_into(js_sys::Object::new()),
        }
    }
}
pub struct FetchOptionsBuilder {
    inner: FetchOptions,
}
impl FetchOptionsBuilder {
    pub fn retries(self, val: f64) -> Self {
        self.inner.set_retries(val);
        self
    }
    pub fn timeout(self, val: f64) -> Self {
        self.inner.set_timeout(val);
        self
    }
    pub fn priority(self, val: &str) -> Self {
        self.inner.set_priority(val);
        self
    }
    pub fn build(self) -> FetchOptions {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Response , extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type ResponseExt;
    #[doc = " Parse the body as JSON and return a typed result."]
    #[wasm_bindgen(method, catch, js_name = "jsonExt")]
    pub async fn json_ext(this: &ResponseExt) -> Result<JsValue, JsValue>;
    #[doc = " Get the response body as a Uint8Array."]
    #[wasm_bindgen(method, catch)]
    pub async fn bytes(this: &ResponseExt) -> Result<ArrayBuffer, JsValue>;
    #[doc = " Whether the response was served from cache."]
    #[wasm_bindgen(method, getter)]
    pub fn cached(this: &ResponseExt) -> bool;
    #[doc = " Timing info in milliseconds."]
    #[wasm_bindgen(method, getter)]
    pub fn timing(this: &ResponseExt) -> f64;
}
#[wasm_bindgen]
extern "C" {
    #[doc = " Perform a fetch with extended options, returning an extended response."]
    #[wasm_bindgen(catch)]
    pub async fn fetch(input: &Request) -> Result<ResponseExt, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[doc = " Perform a fetch with extended options, returning an extended response."]
    #[wasm_bindgen(catch, js_name = "fetch")]
    pub async fn fetch_with_str(input: &str) -> Result<ResponseExt, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[doc = " Perform a fetch with extended options, returning an extended response."]
    #[wasm_bindgen(catch, js_name = "fetch")]
    pub async fn fetch_with_request_and_init(
        input: &Request,
        init: &FetchOptions,
    ) -> Result<ResponseExt, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[doc = " Perform a fetch with extended options, returning an extended response."]
    #[wasm_bindgen(catch, js_name = "fetch")]
    pub async fn fetch_with_str_and_init(
        input: &str,
        init: &FetchOptions,
    ) -> Result<ResponseExt, JsValue>;
}
