#[allow(dead_code)]
use ::web_sys::Blob;
#[allow(dead_code)]
use ::web_sys::ReadableStream;
#[allow(dead_code)]
use ::web_sys::Request;
#[allow(dead_code)]
use ::web_sys::Response;
#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type Writable;
    #[wasm_bindgen(method)]
    pub fn write(this: &Writable, data: &str) -> bool;
    #[wasm_bindgen(method, catch, js_name = "write")]
    pub fn try_write(this: &Writable, data: &str) -> Result<bool, JsValue>;
}
#[allow(dead_code)]
pub type WritableStream = Writable;
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Priority {
    Low = -1i32,
    Normal = 0i32,
    High = 1i32,
}
#[allow(dead_code)]
pub type StringOrNumber = JsValue;
#[allow(dead_code)]
pub type BodyInit = JsValue;
#[wasm_bindgen]
extern "C" {
    pub fn send(body: &ReadableStream);
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "send")]
    pub fn try_send(body: &ReadableStream) -> Result<(), JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "send")]
    pub fn send_with_str(body: &str);
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "send")]
    pub fn try_send_with_str(body: &str) -> Result<(), JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "send")]
    pub fn send_with_array_buffer(body: &ArrayBuffer);
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "send")]
    pub fn try_send_with_array_buffer(body: &ArrayBuffer) -> Result<(), JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "send")]
    pub fn send_with_blob(body: &Blob);
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "send")]
    pub fn try_send_with_blob(body: &Blob) -> Result<(), JsValue>;
}
#[allow(dead_code)]
pub type RequestInfo = JsValue;
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch)]
    pub async fn fetch(input: &str) -> Result<Response, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "fetch")]
    pub async fn fetch_with_request(input: &Request) -> Result<Response, JsValue>;
}
