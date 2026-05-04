#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type AllRequired;
    #[doc = " The thing's name."]
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &AllRequired) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_name(this: &AllRequired, val: &str);
    #[doc = " The thing's count."]
    #[wasm_bindgen(method, getter)]
    pub fn count(this: &AllRequired) -> f64;
    #[wasm_bindgen(method, setter)]
    pub fn set_count(this: &AllRequired, val: f64);
}
impl AllRequired {
    #[doc = " * `name` - The thing's name."]
    #[doc = " * `count` - The thing's count."]
    pub fn new(name: &str, count: f64) -> AllRequired {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_name(name);
        inner.set_count(count);
        inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type HasOptional;
    #[doc = " Required."]
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &HasOptional) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_name(this: &HasOptional, val: &str);
    #[doc = " Optional."]
    #[wasm_bindgen(method, getter)]
    pub fn count(this: &HasOptional) -> Option<f64>;
    #[wasm_bindgen(method, setter)]
    pub fn set_count(this: &HasOptional, val: f64);
}
impl HasOptional {
    #[doc = " * `name` - Required."]
    pub fn new(name: &str) -> HasOptional {
        Self::builder(name).build()
    }
    #[doc = " * `name` - Required."]
    pub fn builder(name: &str) -> HasOptionalBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_name(name);
        HasOptionalBuilder { inner }
    }
}
pub struct HasOptionalBuilder {
    inner: HasOptional,
}
impl HasOptionalBuilder {
    pub fn count(self, val: f64) -> Self {
        self.inner.set_count(val);
        self
    }
    pub fn build(self) -> HasOptional {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type AllOptional;
    #[doc = " Optional."]
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &AllOptional) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_name(this: &AllOptional, val: &str);
    #[doc = " Optional."]
    #[wasm_bindgen(method, getter)]
    pub fn count(this: &AllOptional) -> Option<f64>;
    #[wasm_bindgen(method, setter)]
    pub fn set_count(this: &AllOptional, val: f64);
}
impl AllOptional {
    pub fn new() -> AllOptional {
        Self::builder().build()
    }
    pub fn builder() -> AllOptionalBuilder {
        AllOptionalBuilder {
            inner: JsCast::unchecked_into(js_sys::Object::new()),
        }
    }
}
pub struct AllOptionalBuilder {
    inner: AllOptional,
}
impl AllOptionalBuilder {
    pub fn name(self, val: &str) -> Self {
        self.inner.set_name(val);
        self
    }
    pub fn count(self, val: f64) -> Self {
        self.inner.set_count(val);
        self
    }
    pub fn build(self) -> AllOptional {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type SingleRequired;
    #[doc = " The id."]
    #[wasm_bindgen(method, getter)]
    pub fn id(this: &SingleRequired) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_id(this: &SingleRequired, val: &str);
}
impl SingleRequired {
    #[doc = " * `id` - The id."]
    pub fn new(id: &str) -> SingleRequired {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_id(id);
        inner
    }
}
