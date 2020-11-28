use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::{closure::Closure, JsValue};
use web_sys::{CloseEvent, ErrorEvent, Event};

use crate::core::WsCore;
use crate::emitter::Emitter;
use crate::simple_rpc::RPCSubscriber;
use crate::{Websocket, WsMessage};

pub struct WsFactory {
    pub url: Rc<Cow<'static, str>>,
    pub on_message: Option<Rc<RefCell<dyn FnMut(WsMessage)>>>,
    pub on_open: Option<Rc<RefCell<dyn FnMut(Event)>>>,
    pub on_error: Option<Rc<RefCell<dyn FnMut(ErrorEvent)>>>,
    pub on_close: Option<Rc<RefCell<dyn FnMut(CloseEvent)>>>,
    pub reconnect: Option<Rc<RefCell<ReconnectConfig>>>,
    pub is_closing: Rc<RefCell<bool>>,
    pub emitter: Option<Rc<RefCell<Emitter>>>,
    pub rpc_subscriber: Option<Rc<RefCell<RPCSubscriber>>>,
}

impl WsFactory {
    pub(crate) fn new(url: Cow<'static, str>) -> Self {
        Self {
            url: Rc::new(url),
            on_message: None,
            on_open: None,
            on_error: None,
            on_close: None,
            reconnect: Some(Rc::new(RefCell::new(ReconnectConfig::default()))),
            is_closing: Rc::new(RefCell::new(false)),
            emitter: Some(Rc::new(RefCell::new(Emitter::new()))),
            rpc_subscriber: Some(Rc::new(RefCell::new(RPCSubscriber::new()))),
        }
    }

    pub fn build(self) -> Result<Websocket, JsValue> {
        let websocket_ref = Rc::new(RefCell::new(WsCore::build_new_websocket(&self.url)?));
        let core = WsCore::new(self, websocket_ref);
        Ok(Websocket::new(core))
    }

    pub fn on_message(mut self, f: impl FnMut(WsMessage) + 'static) -> Self {
        self.on_message = Some(Rc::new(RefCell::new(f)));
        self
    }

    pub fn on_open(mut self, f: impl FnMut(Event) + 'static) -> Self {
        self.on_open = Some(Rc::new(RefCell::new(f)));
        self
    }

    pub fn on_error(mut self, f: impl FnMut(ErrorEvent) + 'static) -> Self {
        self.on_error = Some(Rc::new(RefCell::new(f)));
        self
    }

    pub fn on_close(mut self, f: impl FnMut(CloseEvent) + 'static) -> Self {
        self.on_close = Some(Rc::new(RefCell::new(f)));
        self
    }

    pub fn reconnect(mut self, cfg: ReconnectConfig) -> Self {
        self.reconnect = Some(Rc::new(RefCell::new(cfg)));
        self
    }

    pub fn no_reconnect(mut self) -> Self {
        self.reconnect = None;
        self
    }
}

#[derive(Debug)]
pub struct ReconnectConfig {
    is_reconnecting: bool,
    retry_closure: Rc<RefCell<Option<Closure<dyn FnMut() + 'static>>>>,
}

impl ReconnectConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_reconnecting(&self) -> bool {
        self.is_reconnecting
    }

    pub fn reset(&mut self) {
        self.is_reconnecting = false;
    }

    pub fn set_retry_cb(&self, cb: Closure<dyn FnMut() + 'static>) {
        self.retry_closure.borrow_mut().replace(cb);
    }
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        let retry_closure = Rc::new(RefCell::new(None));
        ReconnectConfig {
            is_reconnecting: false,
            retry_closure,
        }
    }
}
