#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{JsString, Number, Object};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use ts_gen_integration_tests::generic_type_level::{
    load_value_typed, socket_event_map, Bag, Chained, Event, EventTarget, Labelled, MessageEvent,
    PingTarget, Socket, SocketEventMap, TransformStream,
};

fn assert_eq<T: Eq>() {}

// `Eq` holds for a generic type at its `JsValue` default, for a subclass of
// a generic parent written with arguments, and for a child of
// `Record<string, unknown>` (`Object<JsValue>`).
#[allow(dead_code)]
fn every_type_is_eq() {
    assert_eq::<TransformStream>();
    assert_eq::<Labelled<JsValue>>();
    assert_eq::<PingTarget>();
    assert_eq::<Bag>();
}

// An async static and an async free function resolving to a bare type
// parameter bind it `FromWasmAbi`, which `JsFuture<T>` needs to convert
// the settled value, and resolve to the caller's choice of `T`.
#[allow(dead_code)]
async fn async_bare_param_returns() -> Result<(), JsValue> {
    let _: JsString = Labelled::<JsValue>::load_typed::<JsString>().await?;
    let _: Number = load_value_typed::<Number>().await?;
    Ok(())
}

// A constructor fills an unmentioned parameter whose default names an
// earlier unmentioned one (`B = A`) with that one's default, so `new`
// declares only the parameter its argument mentions.
#[allow(dead_code)]
fn chained_defaults_resolve(c: &Number) -> Result<Chained<JsString, JsString, Number>, JsValue> {
    Chained::new(c)
}

#[wasm_bindgen_test]
fn generic_handles_compare_by_identity() {
    let object = Object::new();
    let a: TransformStream = JsValue::from(&object).unchecked_into();
    let b: TransformStream = JsValue::from(&object).unchecked_into();
    let other: TransformStream = JsValue::from(Object::new()).unchecked_into();
    assert!(a == b);
    assert!(a != other);
}

// A `keyof` parameter keeps its relation in the `_typed` twin: the marker
// names the key, and the listener takes that key's event. A subclass
// reaches the twin at its own map, and the plain name still takes any
// string.
#[wasm_bindgen_test]
fn typed_listener_receives_its_event() {
    let socket: Socket = js_sys::eval("new EventTarget()").unwrap().unchecked_into();
    let data = Rc::new(RefCell::new(None));
    let sink = data.clone();
    let on_message = Closure::<dyn FnMut(MessageEvent)>::new(move |event: MessageEvent| {
        *sink.borrow_mut() = Some(event.data());
    });
    socket
        .add_event_listener_typed::<socket_event_map::Message>(on_message.as_ref().unchecked_ref());

    let closed = Rc::new(RefCell::new(None));
    let sink = closed.clone();
    let on_close = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        *sink.borrow_mut() = Some(event.type_());
    });
    let target: &EventTarget<SocketEventMap> = socket.unchecked_ref();
    target.add_event_listener_typed::<socket_event_map::Close>(on_close.as_ref().unchecked_ref());

    let message: MessageEvent = js_sys::eval("new MessageEvent('message', { data: 'hi' })")
        .unwrap()
        .unchecked_into();
    socket.dispatch_event(&message);
    assert_eq!(data.borrow().as_deref(), Some("hi"));
    assert_eq!(*closed.borrow(), None);

    let close: Event = js_sys::eval("new Event('close')").unwrap().unchecked_into();
    socket.dispatch_event(&close);
    assert_eq!(closed.borrow().as_deref(), Some("close"));

    let any = Closure::<dyn FnMut(Event)>::new(|_: Event| {});
    socket.add_event_listener("open", any.as_ref().unchecked_ref());
}
