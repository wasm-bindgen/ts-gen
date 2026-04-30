#[allow(dead_code)]
use ::web_sys::AbortSignal;
#[allow(dead_code)]
use ::web_sys::Headers;
#[allow(unused_imports)]
use js_sys::*;
#[allow(unused_imports)]
use wasm_bindgen::prelude::*;
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type StringMap;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type NumberIndexed;
    #[wasm_bindgen(method, getter)]
    pub fn length(this: &NumberIndexed) -> f64;
    #[wasm_bindgen(method, setter)]
    pub fn set_length(this: &NumberIndexed, val: f64);
}
impl NumberIndexed {
    pub fn new(length: f64) -> NumberIndexed {
        Self::builder(length).build()
    }
    pub fn builder(length: f64) -> NumberIndexedBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_length(length);
        NumberIndexedBuilder { inner }
    }
}
pub struct NumberIndexedBuilder {
    inner: NumberIndexed,
}
impl NumberIndexedBuilder {
    pub fn build(self) -> NumberIndexed {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type MixedWithIndex;
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &MixedWithIndex) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_name(this: &MixedWithIndex, val: &str);
}
impl MixedWithIndex {
    pub fn new(name: &str) -> MixedWithIndex {
        Self::builder(name).build()
    }
    pub fn builder(name: &str) -> MixedWithIndexBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_name(name);
        MixedWithIndexBuilder { inner }
    }
}
pub struct MixedWithIndexBuilder {
    inner: MixedWithIndex,
}
impl MixedWithIndexBuilder {
    pub fn build(self) -> MixedWithIndex {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type HasName;
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &HasName) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_name(this: &HasName, val: &str);
}
impl HasName {
    pub fn new(name: &str) -> HasName {
        Self::builder(name).build()
    }
    pub fn builder(name: &str) -> HasNameBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_name(name);
        HasNameBuilder { inner }
    }
}
pub struct HasNameBuilder {
    inner: HasName,
}
impl HasNameBuilder {
    pub fn build(self) -> HasName {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type HasAge;
    #[wasm_bindgen(method, getter)]
    pub fn age(this: &HasAge) -> f64;
    #[wasm_bindgen(method, setter)]
    pub fn set_age(this: &HasAge, val: f64);
}
impl HasAge {
    pub fn new(age: f64) -> HasAge {
        Self::builder(age).build()
    }
    pub fn builder(age: f64) -> HasAgeBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_age(age);
        HasAgeBuilder { inner }
    }
}
pub struct HasAgeBuilder {
    inner: HasAge,
}
impl HasAgeBuilder {
    pub fn build(self) -> HasAge {
        self.inner
    }
}
#[allow(dead_code)]
pub type Person = JsValue;
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type Serializable;
    #[wasm_bindgen(method)]
    pub fn serialize(this: &Serializable) -> String;
    #[wasm_bindgen(method, catch, js_name = "serialize")]
    pub fn try_serialize(this: &Serializable) -> Result<String, JsValue>;
}
#[allow(dead_code)]
pub type SerializablePerson = JsValue;
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum Direction {
    Up = 0u32,
    Down = 1u32,
    Left = 2u32,
    Right = 3u32,
}
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum HttpStatus {
    Ok = 200u32,
    NotFound = 404u32,
    InternalServerError = 500u32,
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type GlobalMixin;
    #[wasm_bindgen(method, js_name = "customMethod")]
    pub fn custom_method(this: &GlobalMixin);
    #[wasm_bindgen(method, catch, js_name = "customMethod")]
    pub fn try_custom_method(this: &GlobalMixin) -> Result<(), JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "globalHelper")]
    pub fn global_helper(x: f64) -> String;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "globalHelper")]
    pub fn try_global_helper(x: f64) -> Result<String, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(thread_local_v2, js_name = "GLOBAL_VERSION")]
    pub static global_version: String;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type DefaultProcessor;
    #[wasm_bindgen(constructor, catch)]
    pub fn new(config: &Object) -> Result<DefaultProcessor, JsValue>;
    #[wasm_bindgen(method, catch)]
    pub async fn process(this: &DefaultProcessor, input: &str) -> Result<String, JsValue>;
    #[wasm_bindgen(method, getter)]
    pub fn name(this: &DefaultProcessor) -> String;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_name = "createProcessor")]
    pub fn create_processor(name: &str) -> DefaultProcessor;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "createProcessor")]
    pub fn try_create_processor(name: &str) -> Result<DefaultProcessor, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type TreeNode;
    #[wasm_bindgen(method, getter)]
    pub fn value(this: &TreeNode) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_value(this: &TreeNode, val: &str);
    #[wasm_bindgen(method, getter)]
    pub fn children(this: &TreeNode) -> Array<TreeNode>;
    #[wasm_bindgen(method, setter)]
    pub fn set_children(this: &TreeNode, val: &Array<TreeNode>);
    #[wasm_bindgen(method, getter)]
    pub fn parent(this: &TreeNode) -> Option<TreeNode>;
    #[wasm_bindgen(method, setter)]
    pub fn set_parent(this: &TreeNode, val: &TreeNode);
}
impl TreeNode {
    pub fn new(value: &str, children: &Array<TreeNode>) -> TreeNode {
        Self::builder(value, children).build()
    }
    pub fn builder(value: &str, children: &Array<TreeNode>) -> TreeNodeBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_value(value);
        inner.set_children(children);
        TreeNodeBuilder { inner }
    }
}
pub struct TreeNodeBuilder {
    inner: TreeNode,
}
impl TreeNodeBuilder {
    pub fn parent(self, val: &TreeNode) -> Self {
        self.inner.set_parent(val);
        self
    }
    pub fn build(self) -> TreeNode {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type LinkedList;
    #[wasm_bindgen(method, getter)]
    pub fn data(this: &LinkedList) -> JsValue;
    #[wasm_bindgen(method, setter)]
    pub fn set_data(this: &LinkedList, val: &JsValue);
    #[wasm_bindgen(method, getter)]
    pub fn next(this: &LinkedList) -> Option<LinkedList>;
    #[wasm_bindgen(method, setter)]
    pub fn set_next(this: &LinkedList, val: &LinkedList);
    #[wasm_bindgen(method, setter, js_name = "next")]
    pub fn set_next_with_null(this: &LinkedList, val: &Null);
}
impl LinkedList {
    pub fn new(data: &JsValue, next: Option<&LinkedList>) -> LinkedList {
        Self::builder(data, next).build()
    }
    pub fn builder(data: &JsValue, next: Option<&LinkedList>) -> LinkedListBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_data(data);
        inner.set_next(next);
        LinkedListBuilder { inner }
    }
}
pub struct LinkedListBuilder {
    inner: LinkedList,
}
impl LinkedListBuilder {
    pub fn build(self) -> LinkedList {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type Iterable;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type AsyncIterable;
}
#[wasm_bindgen]
extern "C" {
    pub fn parse(input: &str, reviver: &Function) -> Object;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "parse")]
    pub fn try_parse(input: &str, reviver: &Function) -> Result<Object, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    pub fn stringify(value: &JsValue, replacer: &Function, space: f64) -> String;
}
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(catch, js_name = "stringify")]
    pub fn try_stringify(
        value: &JsValue,
        replacer: &Function,
        space: f64,
    ) -> Result<String, JsValue>;
}
#[wasm_bindgen]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum SignedValues {
    NegativeOne = -1i32,
    Zero = 0i32,
    One = 1i32,
    Max = 2147483647i32,
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Serializable , extends = GlobalMixin , extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type MultiExtend;
    #[wasm_bindgen(method, getter)]
    pub fn id(this: &MultiExtend) -> String;
    #[wasm_bindgen(method, setter)]
    pub fn set_id(this: &MultiExtend, val: &str);
}
impl MultiExtend {
    pub fn new(id: &str) -> MultiExtend {
        Self::builder(id).build()
    }
    pub fn builder(id: &str) -> MultiExtendBuilder {
        let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
        inner.set_id(id);
        MultiExtendBuilder { inner }
    }
}
pub struct MultiExtendBuilder {
    inner: MultiExtend,
}
impl MultiExtendBuilder {
    pub fn build(self) -> MultiExtend {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type EventTarget;
    #[wasm_bindgen(method, js_name = "addEventListener")]
    pub fn add_event_listener(this: &EventTarget, r#type: &str, listener: &Function);
    #[wasm_bindgen(method, catch, js_name = "addEventListener")]
    pub fn try_add_event_listener(
        this: &EventTarget,
        r#type: &str,
        listener: &Function,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(method, js_name = "removeEventListener")]
    pub fn remove_event_listener(this: &EventTarget, r#type: &str, listener: &Function);
    #[wasm_bindgen(method, catch, js_name = "removeEventListener")]
    pub fn try_remove_event_listener(
        this: &EventTarget,
        r#type: &str,
        listener: &Function,
    ) -> Result<(), JsValue>;
    #[wasm_bindgen(method, js_name = "dispatchEvent")]
    pub fn dispatch_event(this: &EventTarget, event: &Object) -> bool;
    #[wasm_bindgen(method, catch, js_name = "dispatchEvent")]
    pub fn try_dispatch_event(this: &EventTarget, event: &Object) -> Result<bool, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type EventEmitter;
    #[wasm_bindgen(constructor, catch)]
    pub fn new() -> Result<EventEmitter, JsValue>;
    # [wasm_bindgen (static_method_of = EventEmitter , js_name = "listenerCount")]
    pub fn listener_count(emitter: &EventEmitter, event: &str) -> f64;
    # [wasm_bindgen (static_method_of = EventEmitter , catch , js_name = "listenerCount")]
    pub fn try_listener_count(emitter: &EventEmitter, event: &str) -> Result<f64, JsValue>;
    #[wasm_bindgen(method)]
    pub fn on(this: &EventEmitter, event: &str, listener: &Function) -> JsValue;
    #[wasm_bindgen(method, catch, js_name = "on")]
    pub fn try_on(
        this: &EventEmitter,
        event: &str,
        listener: &Function,
    ) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(method, variadic)]
    pub fn emit(this: &EventEmitter, event: &str, args: &[JsValue]) -> bool;
    #[wasm_bindgen(method, variadic, catch, js_name = "emit")]
    pub fn try_emit(this: &EventEmitter, event: &str, args: &[JsValue]) -> Result<bool, JsValue>;
    #[wasm_bindgen(method, js_name = "removeAllListeners")]
    pub fn remove_all_listeners(this: &EventEmitter) -> JsValue;
    #[wasm_bindgen(method, catch, js_name = "removeAllListeners")]
    pub fn try_remove_all_listeners(this: &EventEmitter) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(method, js_name = "removeAllListeners")]
    pub fn remove_all_listeners_with_event(this: &EventEmitter, event: &str) -> JsValue;
    #[wasm_bindgen(method, catch, js_name = "removeAllListeners")]
    pub fn try_remove_all_listeners_with_event(
        this: &EventEmitter,
        event: &str,
    ) -> Result<JsValue, JsValue>;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type Storage;
    #[wasm_bindgen(method, js_name = "getItem")]
    pub fn get_item(this: &Storage, key: &str) -> Option<String>;
    #[wasm_bindgen(method, catch, js_name = "getItem")]
    pub fn try_get_item(this: &Storage, key: &str) -> Result<Option<String>, JsValue>;
    #[wasm_bindgen(method, js_name = "setItem")]
    pub fn set_item(this: &Storage, key: &str, value: &str);
    #[wasm_bindgen(method, catch, js_name = "setItem")]
    pub fn try_set_item(this: &Storage, key: &str, value: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(method, js_name = "removeItem")]
    pub fn remove_item(this: &Storage, key: &str);
    #[wasm_bindgen(method, catch, js_name = "removeItem")]
    pub fn try_remove_item(this: &Storage, key: &str) -> Result<(), JsValue>;
    #[wasm_bindgen(method)]
    pub fn clear(this: &Storage);
    #[wasm_bindgen(method, catch, js_name = "clear")]
    pub fn try_clear(this: &Storage) -> Result<(), JsValue>;
    #[wasm_bindgen(method, getter)]
    pub fn length(this: &Storage) -> f64;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type Cache;
    #[wasm_bindgen(method, catch)]
    pub async fn get(
        this: &Cache,
        key: &str,
    ) -> Result<Option<Map<JsString, Array<JsString>>>, JsValue>;
    #[wasm_bindgen(method, catch)]
    pub async fn set(
        this: &Cache,
        key: &str,
        value: &Map<JsString, Array<JsString>>,
    ) -> Result<(), JsValue>;
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type FetchOptions;
    #[wasm_bindgen(method, getter)]
    pub fn method(this: &FetchOptions) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_method(this: &FetchOptions, val: &str);
    #[wasm_bindgen(method, getter)]
    pub fn headers(this: &FetchOptions) -> Option<JsValue>;
    #[wasm_bindgen(method, setter)]
    pub fn set_headers(this: &FetchOptions, val: &Headers);
    #[wasm_bindgen(method, setter, js_name = "headers")]
    pub fn set_headers_with_record(this: &FetchOptions, val: &Object<JsString>);
    #[wasm_bindgen(method, getter)]
    pub fn body(this: &FetchOptions) -> Option<JsValue>;
    #[wasm_bindgen(method, setter)]
    pub fn set_body(this: &FetchOptions, val: &str);
    #[wasm_bindgen(method, setter, js_name = "body")]
    pub fn set_body_with_array_buffer(this: &FetchOptions, val: &ArrayBuffer);
    #[wasm_bindgen(method, setter, js_name = "body")]
    pub fn set_body_with_null(this: &FetchOptions, val: &Null);
    #[wasm_bindgen(method, getter)]
    pub fn redirect(this: &FetchOptions) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_redirect(this: &FetchOptions, val: &str);
    #[wasm_bindgen(method, getter)]
    pub fn signal(this: &FetchOptions) -> Option<AbortSignal>;
    #[wasm_bindgen(method, setter)]
    pub fn set_signal(this: &FetchOptions, val: &AbortSignal);
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
    pub fn method(self, val: &str) -> Self {
        self.inner.set_method(val);
        self
    }
    pub fn headers(self, val: &Headers) -> Self {
        self.inner.set_headers(val);
        self
    }
    pub fn headers_with_record(self, val: &Object<JsString>) -> Self {
        self.inner.set_headers_with_record(val);
        self
    }
    pub fn body(self, val: &str) -> Self {
        self.inner.set_body(val);
        self
    }
    pub fn body_with_array_buffer(self, val: &ArrayBuffer) -> Self {
        self.inner.set_body_with_array_buffer(val);
        self
    }
    pub fn body_with_null(self, val: &Null) -> Self {
        self.inner.set_body_with_null(val);
        self
    }
    pub fn redirect(self, val: &str) -> Self {
        self.inner.set_redirect(val);
        self
    }
    pub fn signal(self, val: &AbortSignal) -> Self {
        self.inner.set_signal(val);
        self
    }
    pub fn build(self) -> FetchOptions {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type SimpleConfig;
    #[wasm_bindgen(method, getter)]
    pub fn verbose(this: &SimpleConfig) -> Option<bool>;
    #[wasm_bindgen(method, setter)]
    pub fn set_verbose(this: &SimpleConfig, val: bool);
}
impl SimpleConfig {
    pub fn new() -> SimpleConfig {
        Self::builder().build()
    }
    pub fn builder() -> SimpleConfigBuilder {
        SimpleConfigBuilder {
            inner: JsCast::unchecked_into(js_sys::Object::new()),
        }
    }
}
pub struct SimpleConfigBuilder {
    inner: SimpleConfig,
}
impl SimpleConfigBuilder {
    pub fn verbose(self, val: bool) -> Self {
        self.inner.set_verbose(val);
        self
    }
    pub fn build(self) -> SimpleConfig {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type NotificationOptions;
    #[wasm_bindgen(method, getter)]
    pub fn body(this: &NotificationOptions) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_body(this: &NotificationOptions, val: &str);
    #[wasm_bindgen(method, getter)]
    pub fn icon(this: &NotificationOptions) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_icon(this: &NotificationOptions, val: &str);
    #[wasm_bindgen(method, getter)]
    pub fn tag(this: &NotificationOptions) -> Option<String>;
    #[wasm_bindgen(method, setter)]
    pub fn set_tag(this: &NotificationOptions, val: &str);
    #[wasm_bindgen(method, getter)]
    pub fn data(this: &NotificationOptions) -> Option<JsValue>;
    #[wasm_bindgen(method, setter)]
    pub fn set_data(this: &NotificationOptions, val: &JsValue);
}
impl NotificationOptions {
    pub fn new() -> NotificationOptions {
        Self::builder().build()
    }
    pub fn builder() -> NotificationOptionsBuilder {
        NotificationOptionsBuilder {
            inner: JsCast::unchecked_into(js_sys::Object::new()),
        }
    }
}
pub struct NotificationOptionsBuilder {
    inner: NotificationOptions,
}
impl NotificationOptionsBuilder {
    pub fn body(self, val: &str) -> Self {
        self.inner.set_body(val);
        self
    }
    pub fn icon(self, val: &str) -> Self {
        self.inner.set_icon(val);
        self
    }
    pub fn tag(self, val: &str) -> Self {
        self.inner.set_tag(val);
        self
    }
    pub fn data(self, val: &JsValue) -> Self {
        self.inner.set_data(val);
        self
    }
    pub fn build(self) -> NotificationOptions {
        self.inner
    }
}
#[wasm_bindgen]
extern "C" {
    # [wasm_bindgen (extends = Object)]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub type MutableWidget;
    #[wasm_bindgen(method, getter)]
    pub fn label(this: &MutableWidget) -> JsValue;
    #[wasm_bindgen(method, setter)]
    pub fn set_label(this: &MutableWidget, val: &str);
    #[wasm_bindgen(method, setter, js_name = "label")]
    pub fn set_label_with_f64(this: &MutableWidget, val: f64);
    #[wasm_bindgen(method, getter)]
    pub fn id(this: &MutableWidget) -> String;
    #[wasm_bindgen(method, getter)]
    pub fn callback(this: &MutableWidget) -> Function;
    #[wasm_bindgen(method, setter)]
    pub fn set_callback(this: &MutableWidget, val: &Function);
}
impl MutableWidget {
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        #[allow(unused_unsafe)]
        unsafe {
            JsValue::from(js_sys::Object::new()).unchecked_into()
        }
    }
}
pub mod my_module {
    use super::*;
    use js_sys::*;
    use wasm_bindgen::prelude::*;
    #[wasm_bindgen(module = "my-module")]
    extern "C" {
        #[wasm_bindgen(catch, js_name = "doWork")]
        pub async fn do_work(input: &str) -> Result<String, JsValue>;
    }
    #[wasm_bindgen(module = "my-module")]
    extern "C" {
        # [wasm_bindgen (extends = Object)]
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub type WorkResult;
        #[wasm_bindgen(method, getter)]
        pub fn success(this: &WorkResult) -> bool;
        #[wasm_bindgen(method, setter)]
        pub fn set_success(this: &WorkResult, val: bool);
        #[wasm_bindgen(method, getter)]
        pub fn data(this: &WorkResult) -> Option<String>;
        #[wasm_bindgen(method, setter)]
        pub fn set_data(this: &WorkResult, val: &str);
    }
    impl WorkResult {
        pub fn new(success: bool) -> WorkResult {
            Self::builder(success).build()
        }
        pub fn builder(success: bool) -> WorkResultBuilder {
            let inner: Self = JsCast::unchecked_into(js_sys::Object::new());
            inner.set_success(success);
            WorkResultBuilder { inner }
        }
    }
    pub struct WorkResultBuilder {
        inner: WorkResult,
    }
    impl WorkResultBuilder {
        pub fn data(self, val: &str) -> Self {
            self.inner.set_data(val);
            self
        }
        pub fn build(self) -> WorkResult {
            self.inner
        }
    }
}
