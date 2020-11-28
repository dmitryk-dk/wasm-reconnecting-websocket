use std::borrow::Cow;
use std::rc::Rc;

use jsonrpc_core::Params;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsValue;
use web_sys::{BinaryType, Event};

use crate::core::WsCore;
use crate::emitter::Payload;
use crate::factory::WsFactory;
use crate::simple_rpc::RPCHandler;

pub mod core;
pub mod emitter;
pub mod factory;
pub mod simple_rpc;
pub mod utils;

#[wasm_bindgen]
extern "C" {
    fn setInterval(closure: &Closure<dyn FnMut()>, time: u32) -> i32;
    fn setTimeout(closure: &Closure<dyn FnMut()>, time: u32);
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen]
pub struct Websocket {
    core: Rc<WsCore>,
}

impl Websocket {
    pub fn new(core: WsCore) -> Self {
        Self {
            core: Rc::new(core),
        }
    }

    pub fn connect<U: Into<Cow<'static, str>>>(url: U) -> WsFactory {
        WsFactory::new(url.into())
    }

    pub fn close(self, code: Option<u16>, reason: Option<String>) -> Result<(), JsValue> {
        self.core.close(code.unwrap_or(1000u16), reason)
    }

    pub fn close_from_drop(&mut self) -> Result<(), JsValue> {
        self.core.close(1000u16, None)
    }

    pub fn send(&self, websocket_message: WsMessage) -> Result<(), JsValue> {
        match websocket_message {
            WsMessage::Text(payload) => {
                self.core.websocket.borrow().send_with_str(payload.as_str())
            }
            WsMessage::Binary(mut payload) => self
                .core
                .websocket
                .borrow()
                .send_with_u8_array(payload.as_mut_slice()),
        }
    }
    pub fn prepare_rpc_request(
        &self,
        method: String,
        rpc_params: Params,
        callback: RPCHandler,
        error_callback: RPCHandler,
    ) -> Option<String> {
        let websocket_core = self.core.clone();
        let factory = websocket_core.factory.clone();
        if !factory.rpc_subscriber.is_none() {
            let raw_rpc_subscriber = factory.rpc_subscriber.as_ref();
            if let Some(rpc_subscriber) = raw_rpc_subscriber {
                let mut rpc_subscriber_ref = rpc_subscriber.borrow_mut();
                let (request_id, raw_request) =
                    rpc_subscriber_ref.prepare_request(method.as_str(), rpc_params);
                rpc_subscriber_ref.set_handler(request_id, callback);
                rpc_subscriber_ref.set_error_handler(request_id, error_callback);
                let rpc_request = serde_json::to_string(&raw_request).unwrap();
                return Some(rpc_request);
            }
        }
        None
    }

    pub fn send_text_rpc(
        &self,
        method: String,
        rpc_params: Params,
        callback: RPCHandler,
        error_callback: RPCHandler,
    ) {
        if let Some(rpc_request) =
            self.prepare_rpc_request(method, rpc_params, callback, error_callback)
        {
            match self.send(WsMessage::Text(rpc_request)) {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }

    pub fn send_binary_rpc(
        &self,
        method: String,
        rpc_params: Params,
        callback: RPCHandler,
        error_callback: RPCHandler,
    ) {
        if let Some(rpc_request) =
            self.prepare_rpc_request(method, rpc_params, callback, error_callback)
        {
            match self.send(WsMessage::Binary(Vec::from(rpc_request))) {
                Ok(_) => {}
                Err(_) => {}
            }
        }
    }

    pub fn url(&self) -> String {
        self.core.websocket.borrow().url()
    }

    pub fn add_listener<H>(&self, handler_name: String, handler: H)
    where
        H: Fn(&Payload) + 'static,
    {
        let websocket_core = self.core.clone();
        let factory = websocket_core.factory.clone();
        let copy_handler_name = handler_name.clone();
        if !factory.emitter.is_none() {
            let emit = factory.emitter.as_ref();
            if let Some(emitter) = emit {
                let mut emitter = emitter.borrow_mut();
                emitter.on(copy_handler_name, Box::new(handler));
            }
        }
    }

    pub fn ready_state(&self) -> ReadyState {
        ReadyState::from(self.core.websocket.borrow().ready_state())
    }

    pub fn set_binary_type(&self) {
        self.core
            .websocket
            .borrow()
            .set_binary_type(BinaryType::Arraybuffer)
    }
}

impl Drop for Websocket {
    fn drop(&mut self) {
        let _ = self.close_from_drop();
    }
}

#[derive(Copy, Clone, Debug)]
pub enum ReadyState {
    Connecting,
    Open,
    Closing,
    Closed,
    Other(u16),
}

impl From<u16> for ReadyState {
    fn from(state: u16) -> Self {
        match state {
            0 => ReadyState::Connecting,
            1 => ReadyState::Open,
            2 => ReadyState::Closing,
            3 => ReadyState::Closed,
            _ => ReadyState::Other(state), // TODO: maybe we just use `unreachable!()` here.
        }
    }
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum WsEvent {
    Open(Event),
    Message(WsMessage),
    Error(Event),
    Close(Event),
}

#[derive(Clone, Debug)]
pub enum WsMessage {
    Text(String),
    Binary(Vec<u8>),
}
