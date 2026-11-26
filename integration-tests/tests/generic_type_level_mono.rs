//! @ts-gen --experimental-generic-mono

#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen_test::*;

use ts_gen_integration_tests::generic_type_level_mono::{
    socket_event_map, Event, EventTarget, MessageEvent, Socket, SocketEventMap,
};

// The `_typed` twin of a keyed member is the same under
// per-monomorphization: the key stays a marker type.
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
