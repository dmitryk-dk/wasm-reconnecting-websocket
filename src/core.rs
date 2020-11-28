use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use std::str;

use js_sys::{JsString, Uint8Array};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasm_bindgen::prelude::*;
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{CloseEvent, ErrorEvent, Event, MessageEvent, WebSocket};

use crate::emitter::Payload;
use crate::factory::WsFactory;
use crate::simple_rpc::RPCSubscriber;

#[wasm_bindgen]
extern "C" {
    fn setInterval(closure: &Closure<dyn FnMut()>, time: u32) -> i32;
    fn setTimeout(closure: &Closure<dyn FnMut()>, time: u32);
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

pub struct WsCore {
    pub factory: Rc<WsFactory>,
    pub websocket: Rc<RefCell<WebSocket>>,
}

impl WsCore {
    pub fn build_new_websocket(url: &Cow<'static, str>) -> Result<WebSocket, JsValue> {
        let websocket = WebSocket::new(url.as_ref())?;
        Ok(websocket)
    }

    pub fn new(factory: WsFactory, websocket: Rc<RefCell<WebSocket>>) -> Self {
        let factory = Rc::new(factory);
        let pinger = Some(Rc::new(RefCell::new(Pinger::new(None))));
        Self::init_new_websocket(factory.clone(), websocket.clone(), pinger.clone());
        Self { factory, websocket }
    }

    pub fn close(&self, code: u16, reason: Option<String>) -> Result<(), JsValue> {
        *self.factory.is_closing.borrow_mut() = true;
        match reason {
            None => self.websocket.borrow().close_with_code(code),
            Some(reason) => self
                .websocket
                .borrow()
                .close_with_code_and_reason(code, reason.as_str()),
        }
    }

    fn init_new_websocket(
        factory: Rc<WsFactory>,
        websocket: Rc<RefCell<WebSocket>>,
        pinger: Option<Rc<RefCell<Pinger>>>,
    ) {
        if let Some(pinger) = pinger.clone() {
            *pinger.borrow_mut() = Pinger::new(Some(websocket.clone()));
        }
        let onmessage = Self::build_onmessage(factory.clone());
        let onopen = Self::build_onopen(factory.clone(), websocket.clone(), pinger.clone());
        let onerror = Self::build_onerror(factory.clone());
        let onclose = Self::build_onclose(factory.clone(), websocket.clone(), pinger.clone());
        {
            let inner_ws = websocket.as_ref().borrow();
            inner_ws.set_onmessage(
                onmessage
                    .as_ref()
                    .map(|closure| closure.as_ref().unchecked_ref()),
            );
            inner_ws.set_onopen(
                onopen
                    .as_ref()
                    .map(|closure| closure.as_ref().unchecked_ref()),
            );
            inner_ws.set_onerror(
                onerror
                    .as_ref()
                    .map(|closure| closure.as_ref().unchecked_ref()),
            );
            inner_ws.set_onclose(
                onclose
                    .as_ref()
                    .map(|closure| closure.as_ref().unchecked_ref()),
            );
        }
        match onmessage {
            Some(closure) => closure.forget(),
            None => (),
        }
        match onopen {
            Some(closure) => closure.forget(),
            None => (),
        }
        match onerror {
            Some(closure) => closure.forget(),
            None => (),
        }
        match onclose {
            Some(closure) => closure.forget(),
            None => (),
        }
    }

    fn schedule_reconnect(closure: &Closure<dyn FnMut()>, timeout: u32) {
        setTimeout(closure, timeout);
    }

    fn build_onmessage(
        factory: Rc<WsFactory>,
    ) -> Option<Closure<dyn FnMut(MessageEvent) + 'static>> {
        // @TODO need thick how to use building on_message
        // Unpack the user supplied value. If none, we have nothing to do.
        // if factory.on_message.is_none() {
        //     return None;
        // }
        // let cb = match factory.on_message.clone() {
        //     None => return None,
        //     Some(cb) => cb.clone(),
        // };
        Some(Closure::wrap(Box::new(move |event: MessageEvent| {
            let event: MessageEvent = event.unchecked_into();
            if let Ok(js_string) = event.data().dyn_into::<JsString>() {
                Self::process_text_message(String::from(js_string), factory.clone());
            } else if let Ok(js_array_buffer) = event.data().dyn_into::<js_sys::ArrayBuffer>() {
                let array = Uint8Array::new(&js_array_buffer).to_vec();
                Self::process_array_message(array, factory.clone());
            } else if let Ok(js_blob_array) = event.data().dyn_into::<web_sys::Blob>() {
                Self::process_blob_message(js_blob_array, factory.clone());
            } else {
                console_log!("type not supported!!!")
            }
            // @TODO will use if we need callback on message
            // if let Some(val) = JsString::try_from(&data) {
            //     let inner_cb = &mut *cb.borrow_mut();
            //     return inner_cb(WsMessage::Text(String::from(val)));
            // }
        })))
    }

    fn build_onopen(
        factory: Rc<WsFactory>,
        websocket: Rc<RefCell<WebSocket>>,
        pinger: Option<Rc<RefCell<Pinger>>>,
    ) -> Option<Closure<dyn FnMut(Event) + 'static>> {
        if factory.on_open.is_none() && factory.reconnect.is_none() {
            return None;
        }
        Some(Closure::wrap(Box::new(move |event: Event| {
            if let Some(reconnect_config) = factory.reconnect.clone() {
                reconnect_config.borrow_mut().reset();
            }
            if let Some(on_open_callback) = factory.on_open.clone() {
                let mut inner_callback = on_open_callback.as_ref().borrow_mut();
                inner_callback(event);
            }
            if let Some(pinger) = pinger.clone() {
                let mut pinger_ref = pinger.as_ref().borrow_mut();
                let ping = Ping { ping: "ping" };
                let ping_data = serde_json::to_string(&ping).unwrap();
                match websocket.borrow().send_with_str(ping_data.as_str()) {
                    Ok(_) => (),
                    Err(err) => console_log!("error on send {:?}", err),
                };
                pinger_ref.ping();
            }
            if let Some(emitter) = factory.emitter.clone() {
                let mut emitter_ref = emitter.as_ref().borrow_mut();
                let handlers = emitter_ref.get_handlers_names();
                for handler in handlers.iter() {
                    let subscribe_data = serde_json::to_string(&Subscribe {
                        subscribe: handler.as_str(),
                    })
                    .unwrap();
                    websocket
                        .borrow()
                        .send_with_str(subscribe_data.as_str())
                        .unwrap();
                }
                emitter_ref.emit(String::from("open"), &Payload::Data(String::from("open")));
            }
        })))
    }

    fn build_onerror(factory: Rc<WsFactory>) -> Option<Closure<dyn FnMut(ErrorEvent) + 'static>> {
        // Unpack the user supplied value. If none, we have nothing to do.
        let on_error_callback = match factory.on_error.clone() {
            None => return None,
            Some(callback) => callback,
        };
        Some(Closure::wrap(Box::new(move |event: ErrorEvent| {
            let event: ErrorEvent = event.unchecked_into();
            let websocket_error_message = event.error();
            if let Some(emitter) = factory.emitter.clone() {
                match websocket_error_message.dyn_into::<JsString>() {
                    Ok(error_message) => {
                        emitter.borrow_mut().emit(
                            String::from("error"),
                            &Payload::Data(String::from(error_message)),
                        );
                    }
                    Err(e) => console_log!("err cast js value: {:?}", e),
                }
            }
            let mut inner_error_callback = on_error_callback.as_ref().borrow_mut();
            inner_error_callback(event);
        })))
    }

    fn build_onclose(
        factory: Rc<WsFactory>,
        websocket: Rc<RefCell<WebSocket>>,
        pinger: Option<Rc<RefCell<Pinger>>>,
    ) -> Option<Closure<dyn FnMut(CloseEvent) + 'static>> {
        if factory.on_close.is_none() && factory.reconnect.is_none() {
            return None;
        }
        Some(Closure::wrap(Box::new(move |event: CloseEvent| {
            // @TODO maybe not needed
            //if *factory.is_closing.borrow() {
            if let Some(reconnect_config) = factory.reconnect.clone() {
                let retry_callback = Self::build_retry_closure(factory.clone(), websocket.clone());
                Self::schedule_reconnect(&retry_callback, 1000u32);
                reconnect_config.borrow_mut().set_retry_cb(retry_callback);
            }
            //}
            if let Some(emitter) = factory.emitter.clone() {
                emitter
                    .borrow_mut()
                    .emit(String::from("close"), &Payload::Data(String::from("close")));
            }
            if let Some(pinger) = pinger.clone() {
                let pinger_ref = pinger.as_ref().borrow_mut();
                let raw_id = pinger_ref.get_interval_id();
                if let Some(id) = raw_id {
                    let id = id.as_ref().borrow();
                    pinger_ref.close_ping(*id);
                }
            };
            if let Some(on_close_callback) = factory.on_close.clone() {
                let mut inner_callback = on_close_callback.as_ref().borrow_mut();
                inner_callback(event);
            }
        })))
    }

    fn build_retry_closure(
        factory: Rc<WsFactory>,
        websocket: Rc<RefCell<WebSocket>>,
    ) -> Closure<dyn FnMut() + 'static> {
        Closure::wrap(Box::new(move || {
            // @TODO will think need this or not
            // if !*factory.is_closing.borrow() {
            //     return;
            // }
            let new_websocket_instance = match Self::build_new_websocket(&factory.url) {
                Ok(websocket) => websocket,
                Err(_) => {
                    let reconnect_config = factory.reconnect.clone().unwrap();
                    let retry_callback =
                        Self::build_retry_closure(factory.clone(), websocket.clone());
                    Self::schedule_reconnect(&retry_callback, 1000u32);
                    reconnect_config.borrow_mut().set_retry_cb(retry_callback);
                    return;
                }
            };
            {
                *websocket.borrow_mut() = new_websocket_instance;
            }
            let pinger = Some(Rc::new(RefCell::new(Pinger::new(None))));
            Self::init_new_websocket(factory.clone(), websocket.clone(), pinger.clone());
        }))
    }

    fn process_text_message(payload: String, factory: Rc<WsFactory>) {
        if let Some(emitter) = factory.emitter.clone() {
            let response: Value =
                serde_json::from_str(payload.as_str()).expect("can't deserialize");
            let end_bytes = payload.find(":").unwrap();
            let handler_name = &payload[..end_bytes].replace("{", "").replace("\"", "");
            let data = response[handler_name].clone();
            if handler_name == "jsonrpc" {
                Self::process_rpc_message(payload, factory.clone());
            } else {
                emitter
                    .borrow_mut()
                    .emit(String::from(handler_name), &Payload::Data(data.to_string()));
            }
        }
    }

    fn process_array_message(payload: Vec<u8>, factory: Rc<WsFactory>) {
        if let Some(emitter) = factory.emitter.clone() {
            let response: Value =
                serde_json::from_slice(&*payload.clone()).expect("can't deserialize");
            match str::from_utf8(&*payload.clone()) {
                Ok(string_payload) => {
                    let end_bytes = string_payload.find(":").unwrap();
                    let handler_name = &string_payload[..end_bytes]
                        .replace("{", "")
                        .replace("\"", "");
                    let data = response[handler_name].clone();
                    if handler_name == "jsonrpc" {
                        Self::process_rpc_message(string_payload.to_string(), factory.clone());
                    } else {
                        emitter
                            .borrow_mut()
                            .emit(String::from(handler_name), &Payload::Data(data.to_string()));
                    }
                }
                Err(err) => {
                    emitter
                        .borrow_mut()
                        .emit(String::from("error"), &Payload::Data(err.to_string()));
                }
            }
        }
    }

    fn process_blob_message(js_blob_array: web_sys::Blob, factory: Rc<WsFactory>) {
        let fr = web_sys::FileReader::new().unwrap();
        let fr_c = fr.clone();
        let factory_ref = factory.clone();
        let onloadend_cb = Closure::wrap(Box::new(move |_e: web_sys::ProgressEvent| {
            let array = js_sys::Uint8Array::new(&fr_c.result().unwrap());
            let array = Uint8Array::new(&array).to_vec();
            Self::process_array_message(array, factory_ref.clone());
        }) as Box<dyn FnMut(web_sys::ProgressEvent)>);
        fr.set_onloadend(Some(onloadend_cb.as_ref().unchecked_ref()));
        fr.read_as_array_buffer(&js_blob_array)
            .expect("blob not readable");
        onloadend_cb.forget();
    }

    fn process_rpc_message(payload: String, factory: Rc<WsFactory>) {
        if let Some(emitter) = factory.emitter.clone() {
            if let Some(rpc_subscriber) = factory.rpc_subscriber.clone() {
                let mut rpc_subscriber_ref = rpc_subscriber.as_ref().borrow_mut();
                let raw_rpc_response = RPCSubscriber::get_response(payload);
                match raw_rpc_response {
                    Ok(rpc_response) => {
                        let request_id = rpc_response.id;
                        match request_id {
                            Some(id) => {
                                let handler = rpc_subscriber_ref.get_handler(id);
                                if let Some(handle) = handler {
                                    handle(rpc_response.result.to_string());
                                }
                            }
                            None => console_log!("this is notification"),
                        }
                    }
                    Err(err) => {
                        let request_id = err.id;
                        match request_id {
                            Some(id) => {
                                let handler = rpc_subscriber_ref.get_error_handler(id);
                                if let Some(handle) = handler {
                                    handle(err.msg.to_string());
                                }
                            }
                            None => console_log!("this is notification"),
                        }
                    }
                }
            }
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Ping<'a> {
    ping: &'a str,
}

#[derive(Serialize, Deserialize)]
struct Subscribe<'a> {
    subscribe: &'a str,
}

struct Pinger {
    websocket: Option<Rc<RefCell<WebSocket>>>,
    interval_id: Option<Rc<RefCell<i32>>>,
}

impl Pinger {
    fn new(websocket: Option<Rc<RefCell<WebSocket>>>) -> Self {
        Self {
            websocket,
            interval_id: Some(Rc::new(RefCell::new(0))),
        }
    }

    fn ping(&mut self) {
        let raw_websocket = self.websocket.clone();
        let closure = Closure::wrap(Box::new(move || {
            let ping = Ping { ping: "ping" };
            let ping_data = serde_json::to_string(&ping).unwrap();
            if let Some(websocket) = raw_websocket.clone() {
                match websocket.borrow_mut().send_with_str(ping_data.as_str()) {
                    Ok(_) => (),
                    Err(err) => console_log!("error send ping: {:?}", err),
                };
            }
        }) as Box<dyn FnMut()>);
        let interval_id = setInterval(&closure, 10_000);
        self.interval_id = Some(Rc::new(RefCell::new(interval_id)));
        closure.forget();
    }

    fn close_ping(&self, interval_id: i32) {
        IntervalHandle {
            interval_id: Some(interval_id),
        };
    }

    fn get_interval_id(&self) -> Option<Rc<RefCell<i32>>> {
        self.interval_id.clone()
    }
}

struct IntervalHandle {
    interval_id: Option<i32>,
}

impl Drop for IntervalHandle {
    fn drop(&mut self) {
        match self.interval_id {
            Some(id) => {
                let window = web_sys::window().unwrap();
                window.clear_interval_with_handle(id);
            }
            None => {
                console_log!("no drop id!!!");
            }
        }
    }
}
